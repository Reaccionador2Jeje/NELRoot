use serde::Deserialize;
use std::{
    fs,
    io::Read,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::core::{config, images};

const MAX_INDEX_BYTES: u64 = 4 * 1024 * 1024;

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

fn fetch_index(url: &str) -> Result<RegistryIndex, Box<dyn std::error::Error>> {
    if !url.starts_with("https://") {
        return Err("registry index must use HTTPS".into());
    }

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent(concat!("NELRoot/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let mut response = client.get(url).send()?.error_for_status()?;
    if response.content_length().is_some_and(|size| size > MAX_INDEX_BYTES) {
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
        return Err(format!("unsupported registry schema version: {}", index.schema_version).into());
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
    let archive = downloads.join(format!("{}-{}-{nonce}.tar.gz", image.name, std::process::id()));

    let result = (|| {
        images::download_archive(&image.url, &archive, &image.sha256)?;
        images::import_archive(&image.name, &archive, &image.architecture)
    })();
    let _ = fs::remove_file(&archive);
    result
}

/// Remove abandoned extraction staging directories without touching registered images.
pub fn clean_staging() -> Result<usize, Box<dyn std::error::Error>> {
    let store = images::image_store_dir();
    if !store.exists() {
        return Ok(0);
    }

    let mut removed = 0;
    for entry in fs::read_dir(store)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') && name.contains(".staging-") && entry.path().is_dir() {
            fs::remove_dir_all(entry.path())?;
            removed += 1;
        }
    }
    Ok(removed)
}
