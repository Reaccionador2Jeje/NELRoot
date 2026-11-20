use std::{fs, path::PathBuf};

pub fn data_dir() -> PathBuf {
    if let Some(root) = std::env::var_os("NELROOT_HOME") {
        return PathBuf::from(root);
    }

    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".nelroot");
    }

    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".nelroot")
}

pub fn initialize() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let directory = data_dir();
    fs::create_dir_all(&directory)?;

    let config_path = directory.join("config.toml");
    if !config_path.exists() {
        fs::write(
            &config_path,
            "# NELRoot configuration\n             data_dir = \"~/.nelroot\"\n             image_catalog = \"images.toml\"\n",
        )?;
    }

    let catalog_path = directory.join("images.toml");
    if !catalog_path.exists() {
        fs::write(&catalog_path, "images = []\n")?;
    }

    Ok(config_path)
}
