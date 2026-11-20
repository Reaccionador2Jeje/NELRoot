//! First-party NELRoot Image Hub metadata format and validation.
//!
//! This module defines the stable JSON contract used by the AtDev-hosted
//! catalog. Network transport and publishing are intentionally separate stages.

use serde::{Deserialize, Serialize};
use std::{collections::HashSet, fs, io::Read, path::Path};

const MAX_METADATA_BYTES: u64 = 8 * 1024 * 1024;
const MAX_CATALOG_ENTRIES: usize = 100_000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HubManifest {
    pub schema_version: u32,
    pub image: HubImage,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HubImage {
    /// Stable identifier, for example "atdev/ubuntu-24.04".
    pub id: String,
    pub name: String,
    pub version: String,
    pub distribution: String,
    pub release: String,
    pub architecture: String,
    #[serde(default)]
    pub description: String,
    pub maintainer: String,
    pub license: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub rootfs: HubRootfs,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HubRootfs {
    /// The first supported Hub archive format is gzip-compressed tar.
    pub format: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub download_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HubCatalog {
    pub schema_version: u32,
    pub generated_at: String,
    pub images: Vec<HubCatalogEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HubCatalogEntry {
    pub id: String,
    pub name: String,
    pub version: String,
    pub distribution: String,
    pub architecture: String,
    pub description: String,
    pub manifest_url: String,
    pub manifest_sha256: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

fn valid_identifier(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 128
        && bytes[0].is_ascii_lowercase()
        && bytes.iter().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(*byte, b'/' | b'.' | b'_' | b'-')
        })
        && !value.starts_with('/')
        && !value.ends_with('/')
        && !value.split('/').any(|part| part.is_empty() || part == "." || part == "..")
}

fn valid_text(value: &str, max_len: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max_len && !value.contains('\0')
}

fn valid_architecture(value: &str) -> bool {
    matches!(
        value,
        "x86_64" | "aarch64" | "arm" | "armv7" | "i686" | "riscv64" | "ppc64le" | "s390x"
    )
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_https_url(value: &str) -> bool {
    reqwest::Url::parse(value).is_ok_and(|url| {
        url.scheme() == "https"
            && url.host_str().is_some_and(|host| !host.is_empty())
            && url.username().is_empty()
            && url.password().is_none()
    })
}

pub fn validate_manifest(manifest: &HubManifest) -> Result<(), String> {
    if manifest.schema_version != 1 {
        return Err(format!(
            "unsupported NELRoot manifest schema version: {}",
            manifest.schema_version
        ));
    }

    let image = &manifest.image;
    if !valid_identifier(&image.id) {
        return Err("image id must be a safe lowercase identifier".into());
    }
    for (field, value, limit) in [
        ("name", image.name.as_str(), 128),
        ("version", image.version.as_str(), 64),
        ("distribution", image.distribution.as_str(), 64),
        ("release", image.release.as_str(), 64),
        ("maintainer", image.maintainer.as_str(), 128),
        ("license", image.license.as_str(), 64),
    ] {
        if !valid_text(value, limit) {
            return Err(format!("image {field} is empty or exceeds its length limit"));
        }
    }
    if !valid_architecture(&image.architecture) {
        return Err(format!("unsupported image architecture: {}", image.architecture));
    }
    if image.description.len() > 2048 || image.description.contains('\0') {
        return Err("image description exceeds its length limit".into());
    }
    if image.tags.len() > 64
        || image.tags.iter().any(|tag| !valid_text(tag, 48))
    {
        return Err("image tags must be non-empty and no longer than 48 bytes (maximum 64 tags)".into());
    }

    if image.rootfs.format != "tar.gz" {
        return Err("unsupported rootfs format; the first Hub schema supports tar.gz".into());
    }
    if image.rootfs.size_bytes == 0 {
        return Err("rootfs size_bytes must be greater than zero".into());
    }
    if !valid_sha256(&image.rootfs.sha256) {
        return Err("rootfs sha256 must contain exactly 64 hexadecimal characters".into());
    }
    if !valid_https_url(&image.rootfs.download_url) {
        return Err("rootfs download_url must be a valid HTTPS URL without credentials".into());
    }
    Ok(())
}

pub fn validate_catalog(catalog: &HubCatalog) -> Result<(), String> {
    if catalog.schema_version != 1 {
        return Err(format!(
            "unsupported NELRoot catalog schema version: {}",
            catalog.schema_version
        ));
    }
    if !valid_text(&catalog.generated_at, 64) {
        return Err("catalog generated_at is missing or invalid".into());
    }
    if catalog.images.len() > MAX_CATALOG_ENTRIES {
        return Err("catalog exceeds the maximum number of entries".into());
    }

    for entry in &catalog.images {
        if !valid_identifier(&entry.id) {
            return Err(format!("catalog contains an invalid image id: {}", entry.id));
        }
        if !valid_text(&entry.name, 128)
            || !valid_text(&entry.version, 64)
            || !valid_text(&entry.distribution, 64)
            || !valid_text(&entry.description, 2048)
        {
            return Err(format!("catalog entry '{}' has invalid text metadata", entry.id));
        }
        if !valid_architecture(&entry.architecture) {
            return Err(format!(
                "catalog entry '{}' has unsupported architecture",
                entry.id
            ));
        }
        if !valid_https_url(&entry.manifest_url) {
            return Err(format!(
                "catalog entry '{}' has an invalid HTTPS manifest URL",
                entry.id
            ));
        }
        if !valid_sha256(&entry.manifest_sha256) {
            return Err(format!(
                "catalog entry '{}' has an invalid manifest SHA-256",
                entry.id
            ));
        }
        if entry.tags.len() > 64 || entry.tags.iter().any(|tag| !valid_text(tag, 48)) {
            return Err(format!("catalog entry '{}' has invalid tags", entry.id));
        }
    }

    let mut identifiers = HashSet::with_capacity(catalog.images.len());
    for entry in &catalog.images {
        if !identifiers.insert(entry.id.as_str()) {
            return Err(format!("catalog contains duplicate image id '{}'", entry.id));
        }
    }
    Ok(())
}

fn read_metadata(path: &Path) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() || metadata.len() > MAX_METADATA_BYTES {
        return Err("Hub metadata must be a regular file no larger than 8 MiB".into());
    }

    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(MAX_METADATA_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_METADATA_BYTES {
        return Err("Hub metadata exceeds the 8 MiB size limit".into());
    }
    Ok(bytes)
}

pub fn load_manifest(path: &Path) -> Result<HubManifest, Box<dyn std::error::Error>> {
    let manifest: HubManifest = serde_json::from_slice(&read_metadata(path)?)?;
    validate_manifest(&manifest).map_err(|error| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("invalid NELRoot manifest: {error}"),
        )
    })?;
    Ok(manifest)
}

pub fn load_catalog(path: &Path) -> Result<HubCatalog, Box<dyn std::error::Error>> {
    let catalog: HubCatalog = serde_json::from_slice(&read_metadata(path)?)?;
    validate_catalog(&catalog).map_err(|error| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("invalid NELRoot catalog: {error}"),
        )
    })?;
    Ok(catalog)
}

pub fn search_catalog<'a>(
    catalog: &'a HubCatalog,
    query: &str,
    architecture: Option<&str>,
) -> Result<Vec<&'a HubCatalogEntry>, String> {
    if query.trim().is_empty() {
        return Err("Hub search query cannot be empty".into());
    }
    if architecture.is_some_and(|value| !valid_architecture(value)) {
        return Err("unsupported architecture filter".into());
    }
    let query = query.trim().to_ascii_lowercase();
    Ok(catalog
        .images
        .iter()
        .filter(|entry| {
            let architecture_matches =
                architecture.is_none_or(|arch| entry.architecture == arch);
            let metadata_matches = [
                entry.id.as_str(),
                entry.name.as_str(),
                entry.distribution.as_str(),
                entry.description.as_str(),
            ]
            .iter()
            .any(|field| field.to_ascii_lowercase().contains(&query));
            let tag_matches = entry
                .tags
                .iter()
                .any(|tag| tag.to_ascii_lowercase().contains(&query));
            architecture_matches && (metadata_matches || tag_matches)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> HubManifest {
        HubManifest {
            schema_version: 1,
            image: HubImage {
                id: "atdev/alpine-3.22".into(),
                name: "Alpine Linux".into(),
                version: "3.22.0".into(),
                distribution: "Alpine".into(),
                release: "3.22".into(),
                architecture: "x86_64".into(),
                description: "Minimal Linux root filesystem".into(),
                maintainer: "AtDev".into(),
                license: "MIT".into(),
                tags: vec!["alpine".into(), "minimal".into()],
                rootfs: HubRootfs {
                    format: "tar.gz".into(),
                    size_bytes: 1024,
                    sha256: "a".repeat(64),
                    download_url: "https://images.atdev.example/alpine-3.22.tar.gz".into(),
                },
            },
        }
    }

    #[test]
    fn accepts_valid_first_party_manifest() {
        assert!(validate_manifest(&manifest()).is_ok());
    }

    #[test]
    fn rejects_unsafe_manifest_ids_and_urls() {
        let mut value = manifest();
        value.image.id = "atdev/../../escape".into();
        assert!(validate_manifest(&value).is_err());

        let mut value = manifest();
        value.image.rootfs.download_url = "http://images.example/rootfs.tar.gz".into();
        assert!(validate_manifest(&value).is_err());

        let mut value = manifest();
        value.image.rootfs.download_url = "https://user:pass@images.example/rootfs.tar.gz".into();
        assert!(validate_manifest(&value).is_err());
    }

    #[test]
    fn rejects_unsupported_formats_architectures_and_digests() {
        let mut value = manifest();
        value.image.rootfs.format = "tar.xz".into();
        assert!(validate_manifest(&value).is_err());

        let mut value = manifest();
        value.image.architecture = "mips".into();
        assert!(validate_manifest(&value).is_err());

        let mut value = manifest();
        value.image.rootfs.sha256 = "bad".into();
        assert!(validate_manifest(&value).is_err());
    }

    #[test]
    fn validates_catalog_uniqueness_and_search() {
        let value = manifest();
        let entry = HubCatalogEntry {
            id: value.image.id.clone(),
            name: value.image.name.clone(),
            version: value.image.version.clone(),
            distribution: value.image.distribution.clone(),
            architecture: value.image.architecture.clone(),
            description: value.image.description.clone(),
            manifest_url: "https://hub.atdev.example/manifests/alpine-3.22.json".into(),
            manifest_sha256: "b".repeat(64),
            tags: value.image.tags.clone(),
        };
        let catalog = HubCatalog {
            schema_version: 1,
            generated_at: "2026-11-20T00:00:00Z".into(),
            images: vec![entry],
        };
        assert!(validate_catalog(&catalog).is_ok());
        assert_eq!(search_catalog(&catalog, "minimal", Some("x86_64")).unwrap().len(), 1);

        let mut duplicate = catalog.clone();
        duplicate.images.push(duplicate.images[0].clone());
        assert!(validate_catalog(&duplicate).is_err());
    }

    #[test]
    fn rejects_catalog_entries_with_bad_manifest_digests() {
        let catalog = HubCatalog {
            schema_version: 1,
            generated_at: "2026-11-20T00:00:00Z".into(),
            images: vec![HubCatalogEntry {
                id: "atdev/alpine".into(),
                name: "Alpine".into(),
                version: "3.22".into(),
                distribution: "Alpine".into(),
                architecture: "x86_64".into(),
                description: "Minimal".into(),
                manifest_url: "https://hub.atdev.example/alpine.json".into(),
                manifest_sha256: "xyz".into(),
                tags: vec![],
            }],
        };
        assert!(validate_catalog(&catalog).is_err());
    }
}
