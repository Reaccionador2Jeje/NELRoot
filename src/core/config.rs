use std::{env, fs, path::PathBuf};

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

pub fn initialize() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let directory = data_dir();
    fs::create_dir_all(&directory)?;

    let config_path = directory.join("config.toml");
    if !config_path.exists() {
        fs::write(
            &config_path,
            "# NELRoot configuration\ndata_dir = \"~/.nelroot\"\nimage_catalog = \"images.toml\"\n",
        )?;
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
    fn data_directory_has_nelroot_suffix() {
        assert!(data_dir().ends_with(".nelroot"));
    }
}
