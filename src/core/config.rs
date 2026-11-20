use std::{env, fs, path::PathBuf};

const DEFAULT_CONFIG: &str = concat!(
    "# NELRoot configuration (schema v1)\n",
    "schema_version = 1\n",
    "log_level = \"info\"\n",
    "\n[paths]\n",
    "image_catalog = \"images.toml\"\n",
    "image_store = \"images\"\n",
    "cache = \"cache\"\n",
    "logs = \"logs\"\n",
    "runtime = \"run\"\n",
);

pub fn data_dir() -> PathBuf {
    if let Some(root) = env::var_os("NELROOT_HOME") {
        return PathBuf::from(root);
    }

    let home = env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    home.join(".nelroot")
}

pub fn cache_dir() -> PathBuf {
    data_dir().join("cache")
}

pub fn logs_dir() -> PathBuf {
    data_dir().join("logs")
}

pub fn runtime_dir() -> PathBuf {
    data_dir().join("run")
}

pub fn config_path() -> PathBuf {
    data_dir().join("config.toml")
}

/// Create NELRoot's complete, user-scoped directory layout and default files.
pub fn initialize() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let directory = data_dir();
    for path in [
        directory.clone(),
        directory.join("images"),
        cache_dir(),
        cache_dir().join("downloads"),
        logs_dir(),
        runtime_dir(),
    ] {
        fs::create_dir_all(path)?;
    }

    let config_path = config_path();
    if !config_path.exists() {
        fs::write(&config_path, DEFAULT_CONFIG)?;
    }

    let catalog_path = directory.join("images.toml");
    if !catalog_path.exists() {
        fs::write(&catalog_path, "images = []\n")?;
    }

    Ok(config_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_directories_are_inside_data_dir() {
        assert!(cache_dir().starts_with(data_dir()));
        assert!(logs_dir().starts_with(data_dir()));
        assert!(runtime_dir().starts_with(data_dir()));
    }

    #[test]
    fn data_directory_has_nelroot_suffix_by_default() {
        if env::var_os("NELROOT_HOME").is_none() {
            assert!(data_dir().ends_with(".nelroot"));
        }
    }
}
