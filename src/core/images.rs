use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

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

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
}

fn looks_like_rootfs(path: &Path) -> bool {
    path.is_dir()
        && ["bin", "etc", "usr", "sbin"]
            .iter()
            .any(|entry| path.join(entry).is_dir())
}

fn save_catalog(catalog: &ImageCatalog) -> Result<(), Box<dyn std::error::Error>> {
    let path = catalog_path();
    let parent = path
        .parent()
        .ok_or("image catalog path has no parent directory")?;
    fs::create_dir_all(parent)?;

    let contents = toml::to_string_pretty(catalog)?;
    fs::write(path, contents)?;
    Ok(())
}

/// Register an existing Linux root filesystem directory without copying or modifying it.
pub fn register_image(
    name: &str,
    rootfs: &Path,
    architecture: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if !valid_name(name) {
        return Err("image name must contain 1-64 ASCII letters, numbers, '-' or '_'".into());
    }
    if architecture.is_empty()
        || !architecture
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
    {
        return Err("architecture must be a non-empty identifier such as aarch64 or x86_64".into());
    }

    let rootfs = rootfs.canonicalize()?;
    if !looks_like_rootfs(&rootfs) {
        return Err(format!(
            "{} does not look like a Linux root filesystem directory",
            rootfs.display()
        )
        .into());
    }

    let mut catalog = load_catalog()?;
    if catalog.images.iter().any(|image| image.name == name) {
        return Err(format!("image '{name}' is already registered").into());
    }

    catalog.images.push(SystemImage {
        name: name.to_owned(),
        architecture: architecture.to_owned(),
        format: "directory".to_owned(),
        source: Some(rootfs.to_string_lossy().into_owned()),
    });
    save_catalog(&catalog)?;
    Ok(())
}

/// Remove an image from the catalog. The root filesystem files are never deleted.
pub fn unregister_image(name: &str) -> Result<bool, Box<dyn std::error::Error>> {
    let mut catalog = load_catalog()?;
    let original_len = catalog.images.len();
    catalog.images.retain(|image| image.name != name);
    if catalog.images.len() == original_len {
        return Ok(false);
    }
    save_catalog(&catalog)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_names_are_restricted_to_safe_identifiers() {
        assert!(valid_name("ubuntu-24_04"));
        assert!(!valid_name("../ubuntu"));
        assert!(!valid_name(""));
        assert!(!valid_name("ubuntu image"));
    }

    #[test]
    fn rootfs_requires_a_standard_linux_directory() {
        let root = std::env::temp_dir().join(format!("nelroot-rootfs-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("etc")).unwrap();
        assert!(looks_like_rootfs(&root));
        fs::remove_dir_all(&root).unwrap();
    }
}
