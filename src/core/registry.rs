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

