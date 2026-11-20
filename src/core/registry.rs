use serde::Deserialize;
use std::{
    fs,
    io::Read,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::core::{config, images};

const MAX_INDEX_BYTES: u64 = 4 * 1024 * 1024;
const STAGING_MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);
const DOWNLOAD_MAX_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);

#[derive(Debug, Clone, Deserialize)]
pub struct RegistryImage {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub architecture: String,
    pub url: String,
    pub sha256: String,
}

#[derive(Debug, Deserialize)]
struct RegistryIndex {
    schema_version: u32,
    images: Vec<RegistryImage>,
}

fn valid_image(image: &RegistryImage) -> bool {
    !image.name.is_empty()
        && image.name.len() <= 64
        && image
            .name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
        && !image.architecture.is_empty()
        && image
            .architecture
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
        && image.url.starts_with("https://")
        && image.sha256.len() == 64
        && image.sha256.chars().all(|character| character.is_ascii_hexdigit())
}

fn fetch_index(url: &str) -> Result<RegistryIndex, Box<dyn std::error::Error>> {
    if !url.starts_with("https://") {
        return Err("registry index must use HTTPS".into());
    }

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(concat!("NELRoot/", env!("CARGO_PKG_VERSION")))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 10 || attempt.url().scheme() != "https" {
                attempt.stop()
            } else {
                attempt.follow()
            }
        }))
        .build()?;
    let mut response = client.get(url).send()?.error_for_status()?;
    if response.url().scheme() != "https" {
        return Err("registry index redirected to a non-HTTPS URL".into());
    }
    if response
        .content_length()
        .is_some_and(|size| size > MAX_INDEX_BYTES)
    {
        return Err("registry index exceeds the 4 MiB size limit".into());
    }

    let mut bytes = Vec::new();
    response
        .by_ref()
        .take(MAX_INDEX_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_INDEX_BYTES {
        return Err("registry index exceeds the 4 MiB size limit".into());
    }

    let index: RegistryIndex = serde_json::from_slice(&bytes)?;
    if index.schema_version != 1 {
        return Err(
            format!("unsupported registry schema version: {}", index.schema_version).into(),
        );
    }
    for image in &index.images {
        if !valid_image(image) {
            return Err(format!(
                "registry contains an invalid entry for image '{}'",
                image.name
            )
            .into());
        }
    }
    for (position, image) in index.images.iter().enumerate() {
        if index.images[position + 1..]
            .iter()
            .any(|other| other.name == image.name && other.architecture == image.architecture)
        {
            return Err(format!(
                "registry contains duplicate entry '{} ({})'",
                image.name, image.architecture
            )
            .into());
        }
    }
    Ok(index)
}

/// Search a remote HTTPS registry index by image name or description.
pub fn search(
    registry_url: &str,
    query: &str,
) -> Result<Vec<RegistryImage>, Box<dyn std::error::Error>> {
    if query.trim().is_empty() {
        return Err("search query cannot be empty".into());
    }
    let index = fetch_index(registry_url)?;
    let query = query.to_ascii_lowercase();
    Ok(index
        .images
        .into_iter()
        .filter(|image| {
            image.name.to_ascii_lowercase().contains(&query)
                || image.description.to_ascii_lowercase().contains(&query)
        })
        .collect())
}

/// Download, verify and import an image listed in a remote registry.
pub fn pull(
    registry_url: &str,
    name: &str,
    architecture: &str,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let index = fetch_index(registry_url)?;
    let image = index
        .images
        .into_iter()
        .find(|image| image.name == name && image.architecture == architecture)
        .ok_or_else(|| format!("image '{name}' for architecture '{architecture}' was not found"))?;

    let downloads = config::cache_dir().join("downloads");
    fs::create_dir_all(&downloads)?;
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let archive = downloads.join(format!(
        "{}-{}-{nonce}.tar.gz",
        image.name,
        std::process::id()
    ));

    let result: Result<PathBuf, Box<dyn std::error::Error>> = (|| {
        images::download_archive(&image.url, &archive, &image.sha256)?;
        images::import_archive(&image.name, &archive, &image.architecture)
    })();
    let _ = fs::remove_file(&archive);
    result
}

fn is_older_than(path: &std::path::Path, age: Duration) -> bool {
    path.metadata()
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .is_some_and(|elapsed| elapsed > age)
}

/// Remove old abandoned staging directories and stale cached downloads.
/// Active or recent work and all registered images are left untouched.
pub fn clean_temporary() -> Result<usize, Box<dyn std::error::Error>> {
    let mut removed = 0;
    let store = images::image_store_dir();
    if store.exists() {
        for entry in fs::read_dir(&store)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let path = entry.path();
            let is_old = is_older_than(&path, STAGING_MAX_AGE);

            if path.is_dir()
                && is_old
                && name.starts_with('.')
                && (name.contains(".staging-")
                    || name.contains(".rootfs-")
                    || name.contains(".update-staging-")
                    || name.contains(".update-rootfs-"))
            {
                fs::remove_dir_all(&path)?;
                removed += 1;
                continue;
            }

            // Recover the old managed image if an update was interrupted after backup.
            if path.is_dir()
                && is_old
                && name.starts_with('.')
                && let Some((image_name, _)) = name
                    .strip_prefix('.')
                    .and_then(|value| value.split_once(".backup-"))
            {
                let destination = store.join(image_name);
                if !destination.exists() {
                    fs::rename(&path, destination)?;
                } else {
                    fs::remove_dir_all(&path)?;
                }
                removed += 1;
            }
        }
    }

    let downloads = config::cache_dir().join("downloads");
    if downloads.exists() {
        for entry in fs::read_dir(downloads)? {
            let entry = entry?;
            if entry.path().is_file() && is_older_than(&entry.path(), DOWNLOAD_MAX_AGE) {
                fs::remove_file(entry.path())?;
                removed += 1;
            }
        }
    }
    Ok(removed)
}


/// Download and verify the latest registry entry, then transactionally update a managed image.
pub fn update(
    registry_url: &str,
    name: &str,
    architecture: &str,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let index = fetch_index(registry_url)?;
    let image = index
        .images
        .into_iter()
        .find(|image| image.name == name && image.architecture == architecture)
        .ok_or_else(|| format!("image '{name}' for architecture '{architecture}' was not found"))?;

    let downloads = config::cache_dir().join("downloads");
    fs::create_dir_all(&downloads)?;
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let archive = downloads.join(format!(
        "{}-update-{}-{nonce}.tar.gz",
        image.name,
        std::process::id()
    ));

    let result: Result<PathBuf, Box<dyn std::error::Error>> = (|| {
        images::download_archive(&image.url, &archive, &image.sha256)?;
        images::update_image_from_archive(&image.name, &archive, &image.architecture)
    })();
    let _ = fs::remove_file(&archive);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn docker_hub_repository_names_are_normalized_and_validated() {
        assert_eq!(normalize_docker_repository("alpine").unwrap(), "library/alpine");
        assert_eq!(normalize_docker_repository("ubuntu/nginx").unwrap(), "ubuntu/nginx");
        assert!(normalize_docker_repository("../alpine").is_err());
        assert!(normalize_docker_repository("one/two/three").is_err());
        assert!(normalize_docker_repository("UPPER/alpine").is_err());
    }

    #[test]
    fn docker_architecture_aliases_map_to_registry_platforms() {
        assert_eq!(docker_architecture("x86_64").unwrap(), "amd64");
        assert_eq!(docker_architecture("aarch64").unwrap(), "arm64");
        assert_eq!(docker_architecture("riscv64").unwrap(), "riscv64");
        assert!(docker_architecture("mips").is_err());
    }

    #[test]
    fn registry_entries_require_safe_names_https_and_sha256() {
        let valid = RegistryImage {
            name: "ubuntu-24_04".into(),
            description: "Ubuntu root filesystem".into(),
            architecture: "aarch64".into(),
            url: "https://example.org/ubuntu.tar.gz".into(),
            sha256: "a".repeat(64),
        };
        assert!(valid_image(&valid));

        let mut invalid = valid.clone();
        invalid.name = "../escape".into();
        assert!(!valid_image(&invalid));

        let mut invalid = valid.clone();
        invalid.url = "http://example.org/ubuntu.tar.gz".into();
        assert!(!valid_image(&invalid));

        let mut invalid = valid;
        invalid.sha256 = "not-a-checksum".into();
        assert!(!valid_image(&invalid));
    }
}



// Docker Hub / OCI Distribution API support. This stage provides remote discovery
// and platform-aware manifest inspection; layer reconstruction is implemented separately.
const DOCKER_HUB_API: &str = "https://hub.docker.com/v2";
const DOCKER_REGISTRY: &str = "https://registry-1.docker.io";
const DOCKER_AUTH: &str = "https://auth.docker.io/token";
const OCI_ACCEPT: &str = "application/vnd.oci.image.index.v1+json, application/vnd.oci.image.manifest.v1+json, application/vnd.docker.distribution.manifest.list.v2+json, application/vnd.docker.distribution.manifest.v2+json";

#[derive(Debug, Clone, Deserialize)]
pub struct DockerHubRepository {
    pub repo_name: String,
    #[serde(default)]
    pub short_description: String,
    #[serde(default)]
    pub star_count: u64,
    #[serde(default)]
    pub pull_count: u64,
    #[serde(default)]
    pub is_official: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DockerHubTag {
    pub name: String,
    #[serde(default)]
    pub last_updated: String,
    #[serde(default)]
    pub full_size: u64,
}

#[derive(Debug, Clone)]
pub struct DockerHubManifest {
    pub repository: String,
    pub reference: String,
    pub digest: String,
    pub media_type: String,
    pub architecture: String,
    pub os: String,
    pub layer_count: usize,
    pub total_layer_bytes: u64,
}

fn docker_http_client() -> Result<reqwest::blocking::Client, Box<dyn std::error::Error>> {
    Ok(reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(60))
        .user_agent(concat!("NELRoot/", env!("CARGO_PKG_VERSION")))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 10 || attempt.url().scheme() != "https" {
                attempt.stop()
            } else {
                attempt.follow()
            }
        }))
        .build()?)
}

fn normalize_docker_repository(repository: &str) -> Result<String, Box<dyn std::error::Error>> {
    let repository = repository.trim().trim_matches('/');
    if repository.is_empty() {
        return Err("Docker Hub repository cannot be empty".into());
    }
    let normalized = if repository.contains('/') {
        repository.to_owned()
    } else {
        format!("library/{repository}")
    };
    let mut parts = normalized.split('/');
    let namespace = parts.next().unwrap_or_default();
    let name = parts.next().unwrap_or_default();
    if parts.next().is_some()
        || [namespace, name].iter().any(|part| {
            part.is_empty()
                || *part == "."
                || *part == ".."
                || part.len() > 255
                || !part
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._-".contains(&byte))
        })
    {
        return Err("Repository must be [namespace/]name using lowercase ASCII letters, digits, '.', '_' or '-', without traversal segments".into());
    }
    Ok(normalized)
}

fn docker_architecture(architecture: &str) -> Result<&'static str, Box<dyn std::error::Error>> {
    match architecture {
        "x86_64" | "amd64" => Ok("amd64"),
        "aarch64" | "arm64" => Ok("arm64"),
        "arm" | "armv7" | "armv7l" => Ok("arm"),
        "i686" | "i386" => Ok("386"),
        "riscv64" => Ok("riscv64"),
        "ppc64le" => Ok("ppc64le"),
        "s390x" => Ok("s390x"),
        _ => Err(format!("unsupported Docker image architecture: {architecture}").into()),
    }
}

/// Search public Docker Hub repositories using its web catalog API.
pub fn docker_hub_search(
    query: &str,
    limit: u16,
) -> Result<Vec<DockerHubRepository>, Box<dyn std::error::Error>> {
    if query.trim().is_empty() {
        return Err("Docker Hub search query cannot be empty".into());
    }
    let limit = limit.clamp(1, 100);
    let client = docker_http_client()?;
    let response = client
        .get(format!("{DOCKER_HUB_API}/search/repositories/"))
        .query(&[("query", query.trim())])
        .query(&[("page_size", limit.to_string())])
        .send()?
        .error_for_status()?;
    if response.url().scheme() != "https" {
        return Err("Docker Hub search redirected to a non-HTTPS URL".into());
    }
    let payload: serde_json::Value = response.json()?;
    let results = payload
        .get("results")
        .and_then(serde_json::Value::as_array)
        .ok_or("Docker Hub search response has no results array")?;
    results
        .iter()
        .map(|entry| serde_json::from_value(entry.clone()).map_err(Into::into))
        .collect()
}

/// List public tags for a Docker Hub repository (up to 100 per request).
pub fn docker_hub_tags(
    repository: &str,
    limit: u16,
) -> Result<Vec<DockerHubTag>, Box<dyn std::error::Error>> {
    let repository = normalize_docker_repository(repository)?;
    let limit = limit.clamp(1, 100);
    let client = docker_http_client()?;
    let response = client
        .get(format!("{DOCKER_HUB_API}/repositories/{repository}/tags/"))
        .query(&[("page_size", limit.to_string())])
        .send()?
        .error_for_status()?;
    if response.url().scheme() != "https" {
        return Err("Docker Hub tags endpoint redirected to a non-HTTPS URL".into());
    }
    let payload: serde_json::Value = response.json()?;
    let results = payload
        .get("results")
        .and_then(serde_json::Value::as_array)
        .ok_or("Docker Hub tags response has no results array")?;
    results
        .iter()
        .map(|entry| serde_json::from_value(entry.clone()).map_err(Into::into))
        .collect()
}

fn docker_bearer_token(
    client: &reqwest::blocking::Client,
    repository: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let response = client
        .get(DOCKER_AUTH)
        .query(&[
            ("service", "registry.docker.io"),
            ("scope", &format!("repository:{repository}:pull")),
        ])
        .send()?
        .error_for_status()?;
    if response.url().scheme() != "https" {
        return Err("Docker Hub token endpoint redirected to a non-HTTPS URL".into());
    }
    let payload: serde_json::Value = response.json()?;
    payload
        .get("token")
        .or_else(|| payload.get("access_token"))
        .and_then(serde_json::Value::as_str)
        .filter(|token| !token.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| "Docker Hub token response did not contain a bearer token".into())
}

/// Resolve a Docker Hub tag or digest and inspect its OCI/Docker manifest for one platform.
pub fn docker_hub_inspect(
    repository: &str,
    reference: &str,
    architecture: &str,
) -> Result<DockerHubManifest, Box<dyn std::error::Error>> {
    let repository = normalize_docker_repository(repository)?;
    if reference.is_empty()
        || reference.len() > 256
        || !reference.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || b"._:-".contains(&byte)
        })
    {
        return Err("image reference contains invalid characters".into());
    }
    let architecture = docker_architecture(architecture)?;
    let client = docker_http_client()?;
    let token = docker_bearer_token(&client, &repository)?;
    let manifest_url = format!("{DOCKER_REGISTRY}/v2/{repository}/manifests/{reference}");
    let response = client
        .get(&manifest_url)
        .bearer_auth(&token)
        .header(reqwest::header::ACCEPT, OCI_ACCEPT)
        .send()?
        .error_for_status()?;
    if response.url().scheme() != "https" {
        return Err("Docker Registry manifest endpoint redirected to a non-HTTPS URL".into());
    }
    let top_digest = response
        .headers()
        .get("docker-content-digest")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let mut payload: serde_json::Value = response.json()?;
    let mut selected_reference = reference.to_owned();

    if let Some(manifests) = payload.get("manifests").and_then(serde_json::Value::as_array) {
        let selected = manifests
            .iter()
            .find(|descriptor| {
                let platform = &descriptor["platform"];
                platform["os"].as_str() == Some("linux")
                    && platform["architecture"].as_str() == Some(architecture)
                    && platform["variant"].as_str().is_none_or(|variant| {
                        architecture != "arm" || variant == "v7"
                    })
            })
            .ok_or_else(|| format!("image has no linux/{architecture} manifest"))?;
        let digest = selected["digest"]
            .as_str()
            .ok_or("selected platform descriptor has no digest")?;
        selected_reference = digest.to_owned();
        let child = client
            .get(format!("{DOCKER_REGISTRY}/v2/{repository}/manifests/{digest}"))
            .bearer_auth(&token)
            .header(reqwest::header::ACCEPT, OCI_ACCEPT)
            .send()?
            .error_for_status()?;
        if child.url().scheme() != "https" {
            return Err("Docker Registry platform manifest redirected to non-HTTPS URL".into());
        }
        payload = child.json()?;
    }

    let layers = payload
        .get("layers")
        .and_then(serde_json::Value::as_array)
        .ok_or("Docker manifest does not contain a layers array")?;
    let total_layer_bytes = layers.iter().try_fold(0_u64, |total, layer| {
        total.checked_add(layer.get("size").and_then(serde_json::Value::as_u64).unwrap_or(0))
            .ok_or("Docker image layer size overflow")
    })?;
    let media_type = payload
        .get("mediaType")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown")
        .to_owned();
    let digest = if selected_reference == reference {
        top_digest
    } else {
        selected_reference.clone()
    };

    Ok(DockerHubManifest {
        repository,
        reference: selected_reference,
        digest,
        media_type,
        architecture: architecture.to_owned(),
        os: "linux".to_owned(),
        layer_count: layers.len(),
        total_layer_bytes,
    })
}
