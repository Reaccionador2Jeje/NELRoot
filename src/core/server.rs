//! Metadata registry for named NELRoot server environments.
//!
//! This first milestone creates and manages environment definitions only.
//! It does not launch guest processes, configure networking, or expose services.

use serde::{Deserialize, Serialize};
use std::{fs, path::{Path, PathBuf}, time::{SystemTime, UNIX_EPOCH}};

use crate::core::{config, images};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServerEnvironment {
    pub name: String,
    pub image: String,
    pub architecture: String,
    pub rootfs: PathBuf,
    pub created_at_unix: u64,
    /// Metadata lifecycle only. "created" does not mean a guest is running.
    pub state: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct ServerCatalog {
    schema_version: u32,
    environments: Vec<ServerEnvironment>,
}

pub fn catalog_path() -> PathBuf {
    config::data_dir().join("servers.toml")
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
}

fn load_catalog() -> Result<ServerCatalog, Box<dyn std::error::Error>> {
    let path = catalog_path();
    if !path.exists() {
        return Ok(ServerCatalog { schema_version: 1, environments: Vec::new() });
    }
    let contents = fs::read_to_string(path)?;
    let catalog: ServerCatalog = toml::from_str(&contents)?;
    if catalog.schema_version != 1 {
        return Err(format!("unsupported server catalog schema version: {}", catalog.schema_version).into());
    }
    Ok(catalog)
}

fn save_catalog(catalog: &ServerCatalog) -> Result<(), Box<dyn std::error::Error>> {
    let path = catalog_path();
    let parent = path.parent().ok_or("server catalog path has no parent directory")?;
    fs::create_dir_all(parent)?;
    let temporary = path.with_extension("toml.tmp");
    fs::write(&temporary, toml::to_string_pretty(catalog)?)?;
    fs::rename(temporary, path)?;
    Ok(())
}

/// Create a named server-environment definition from an already registered image.
/// This does not start the environment or install/launch any guest service.
pub fn create(name: &str, image_name: &str) -> Result<ServerEnvironment, Box<dyn std::error::Error>> {
    if !valid_name(name) {
        return Err("server name must contain 1-64 ASCII letters, numbers, '-' or '_'".into());
    }

    let image_catalog = images::load_catalog()?;
    let image = image_catalog.images.iter()
        .find(|image| image.name == image_name)
        .ok_or_else(|| format!("image '{image_name}' is not registered"))?;
    let rootfs_source = image.source.as_deref()
        .ok_or_else(|| format!("image '{image_name}' has no root filesystem path"))?;
    let rootfs = images::validate_rootfs(Path::new(rootfs_source))?;

    let mut catalog = load_catalog()?;
    if catalog.environments.iter().any(|environment| environment.name == name) {
        return Err(format!("server environment '{name}' already exists").into());
    }

    let created_at_unix = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let environment = ServerEnvironment {
        name: name.to_owned(),
        image: image.name.clone(),
        architecture: image.architecture.clone(),
        rootfs,
        created_at_unix,
        state: "created".to_owned(),
    };
    catalog.environments.push(environment.clone());
    save_catalog(&catalog)?;
    Ok(environment)
}

pub fn list() -> Result<Vec<ServerEnvironment>, Box<dyn std::error::Error>> {
    Ok(load_catalog()?.environments)
}

pub fn inspect(name: &str) -> Result<Option<ServerEnvironment>, Box<dyn std::error::Error>> {
    Ok(load_catalog()?.environments.into_iter().find(|environment| environment.name == name))
}

/// Remove only the environment definition. The registered image and rootfs are never deleted.
pub fn remove(name: &str) -> Result<bool, Box<dyn std::error::Error>> {
    let mut catalog = load_catalog()?;
    let previous_len = catalog.environments.len();
    catalog.environments.retain(|environment| environment.name != name);
    if catalog.environments.len() == previous_len {
        return Ok(false);
    }
    save_catalog(&catalog)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_names_are_restricted_to_safe_identifiers() {
        assert!(valid_name("debian-web"));
        assert!(valid_name("ubuntu_24"));
        assert!(!valid_name(""));
        assert!(!valid_name("../host"));
        assert!(!valid_name("server name"));
        assert!(!valid_name("a".repeat(65).as_str()));
    }
}
