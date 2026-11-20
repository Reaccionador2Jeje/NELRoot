use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemImage {
    pub name: String,
    pub architecture: String,
    pub format: String,
    pub source: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ImageCatalog {
    pub images: Vec<SystemImage>,
}

pub fn catalog_path() -> PathBuf {
    crate::core::config::data_dir().join("images.toml")
}

pub fn load_catalog() -> Result<ImageCatalog, Box<dyn std::error::Error>> {
    let path = catalog_path();
    if !path.exists() {
        return Ok(ImageCatalog::default());
    }

    let contents = fs::read_to_string(path)?;
    Ok(toml::from_str(&contents)?)
}
