use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Write},
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

pub fn image_store_dir() -> PathBuf {
    crate::core::config::data_dir().join("images")
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

fn valid_architecture(architecture: &str) -> bool {
    !architecture.is_empty()
        && architecture
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
}

fn looks_like_rootfs(path: &Path) -> bool {
    path.is_dir()
        && ["bin", "etc", "usr", "sbin"]
            .iter()
            .any(|entry| path.join(entry).is_dir())
}

/// Validate and canonicalize an existing Linux root filesystem directory.
pub fn validate_rootfs(path: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let canonical = path.canonicalize()?;
    if !looks_like_rootfs(&canonical) {
        return Err(format!(
            "{} does not look like a Linux root filesystem directory",
            canonical.display()
        )
        .into());
    }
    Ok(canonical)
}

fn save_catalog(catalog: &ImageCatalog) -> Result<(), Box<dyn std::error::Error>> {
    let path = catalog_path();
    let parent = path
        .parent()
        .ok_or("image catalog path has no parent directory")?;
    fs::create_dir_all(parent)?;

    let contents = toml::to_string_pretty(catalog)?;
    let temporary = path.with_extension("toml.tmp");
    fs::write(&temporary, contents)?;
    fs::rename(temporary, path)?;
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
    if !valid_architecture(architecture) {
        return Err("architecture must be a non-empty identifier such as aarch64 or x86_64".into());
    }

    let rootfs = validate_rootfs(rootfs)?;
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

fn sha256_file(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Download an HTTPS archive and require its published SHA-256 digest.
pub fn download_archive(
    url: &str,
    destination: &Path,
    expected_sha256: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    if !url.starts_with("https://") {
        return Err("image downloads require an HTTPS URL".into());
    }
    if expected_sha256.len() != 64 || !expected_sha256.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("SHA-256 must contain exactly 64 hexadecimal characters".into());
    }
    if destination.exists() {
        return Err(format!("destination already exists: {}", destination.display()).into());
    }
    if let Some(parent) = destination
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(900))
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
    let mut output = File::create(destination)?;
    if let Err(error) = std::io::copy(&mut response, &mut output) {
        let _ = fs::remove_file(destination);
        return Err(error.into());
    }
    output.flush()?;
    drop(output);

    let actual = sha256_file(destination)?;
    if !actual.eq_ignore_ascii_case(expected_sha256) {
        let _ = fs::remove_file(destination);
        return Err(format!(
            "SHA-256 mismatch; expected {expected_sha256}, received {actual}. Download removed."
        )
        .into());
    }
    Ok(actual)
}

/// Extract a gzip-compressed tar archive into NELRoot's managed image store.
/// The archive is rejected if it attempts to write outside the staging directory.
pub fn import_archive(
    name: &str,
    archive_path: &Path,
    architecture: &str,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    if !valid_name(name) {
        return Err("image name must contain 1-64 ASCII letters, numbers, '-' or '_'".into());
    }
    if !valid_architecture(architecture) {
        return Err("architecture must be a non-empty identifier such as aarch64 or x86_64".into());
    }

    let mut catalog = load_catalog()?;
    if catalog.images.iter().any(|image| image.name == name) {
        return Err(format!("image '{name}' is already registered").into());
    }

    let store = image_store_dir();
    fs::create_dir_all(&store)?;
    let destination = store.join(name);
    if destination.exists() {
        return Err(format!("image directory already exists: {}", destination.display()).into());
    }

    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let staging = store.join(format!(".{name}.staging-{}-{nonce}", std::process::id()));
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    fs::create_dir(&staging)?;

    let result = (|| -> Result<PathBuf, Box<dyn std::error::Error>> {
        let input = File::open(archive_path)?;
        let decoder = flate2::read::GzDecoder::new(input);
        let mut archive = tar::Archive::new(decoder);
        for entry in archive.entries()? {
            let mut entry = entry?;
            if !entry.unpack_in(&staging)? {
                return Err("archive contains unsafe paths; extraction was refused".into());
            }
        }

        let root = if looks_like_rootfs(&staging) {
            staging.clone()
        } else {
            let mut candidates = fs::read_dir(&staging)?
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| looks_like_rootfs(path));
            let candidate = candidates
                .next()
                .ok_or("archive does not contain a recognizable Linux root filesystem")?;
            if candidates.next().is_some() {
                return Err("archive contains multiple possible root filesystems".into());
            }
            candidate
        };

        if root != staging {
            let normalized = store.join(format!(
                ".{name}.rootfs-{}-{nonce}",
                std::process::id()
            ));
            fs::rename(&root, &normalized)?;
            fs::remove_dir_all(&staging)?;
            fs::rename(normalized, &destination)?;
        } else {
            fs::rename(&staging, &destination)?;
        }

        let canonical = validate_rootfs(&destination)?;
        catalog.images.push(SystemImage {
            name: name.to_owned(),
            architecture: architecture.to_owned(),
            format: "directory".to_owned(),
            source: Some(canonical.to_string_lossy().into_owned()),
        });
        save_catalog(&catalog)?;
        Ok(canonical)
    })();

    if result.is_err() {
        if staging.exists() {
            let _ = fs::remove_dir_all(&staging);
        }
        let normalized = store.join(format!(
            ".{name}.rootfs-{}-{nonce}",
            std::process::id()
        ));
        if normalized.exists() {
            let _ = fs::remove_dir_all(&normalized);
        }
        // A failed catalog write must not leave an unregistered managed rootfs behind.
        if destination.exists()
            && !load_catalog()
                .map(|catalog| catalog.images.iter().any(|image| image.name == name))
                .unwrap_or(true)
        {
            let _ = fs::remove_dir_all(&destination);
        }
    }
    result
}

/// Replace a NELRoot-managed image with a newly verified rootfs archive.
///
/// Externally registered directories are deliberately not replaceable: NELRoot
/// must never move or delete a user's directory during an image update.
pub fn update_image_from_archive(
    name: &str,
    archive_path: &Path,
    architecture: &str,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    if !valid_name(name) {
        return Err("image name must contain 1-64 ASCII letters, numbers, '-' or '_'".into());
    }
    if !valid_architecture(architecture) {
        return Err("architecture must be a non-empty identifier such as aarch64 or x86_64".into());
    }

    let mut catalog = load_catalog()?;
    let existing = catalog
        .images
        .iter()
        .find(|image| image.name == name)
        .ok_or_else(|| format!("image '{name}' is not registered"))?;
    let destination = image_store_dir().join(name);
    let metadata = fs::symlink_metadata(&destination)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("only real NELRoot-managed image directories can be updated".into());
    }
    let existing_source = existing
        .source
        .as_deref()
        .ok_or("registered image has no root filesystem path")?
        .to_owned();
    if Path::new(&existing_source).canonicalize()? != destination.canonicalize()? {
        return Err(
            "this image points to an external directory; unregister it manually before replacing it"
                .into(),
        );
    }

    let store = image_store_dir();
    fs::create_dir_all(&store)?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let staging = store.join(format!(".{name}.update-staging-{}-{nonce}", std::process::id()));
    let new_rootfs = store.join(format!(".{name}.update-rootfs-{}-{nonce}", std::process::id()));
    let backup = store.join(format!(".{name}.backup-{}-{nonce}", std::process::id()));
    fs::create_dir(&staging)?;

    let mut backup_created = false;
    let result = (|| -> Result<PathBuf, Box<dyn std::error::Error>> {
        let input = File::open(archive_path)?;
        let decoder = flate2::read::GzDecoder::new(input);
        let mut archive = tar::Archive::new(decoder);
        for entry in archive.entries()? {
            let mut entry = entry?;
            if !entry.unpack_in(&staging)? {
                return Err("archive contains unsafe paths; extraction was refused".into());
            }
        }

        let root = if looks_like_rootfs(&staging) {
            staging.clone()
        } else {
            let mut candidates = fs::read_dir(&staging)?
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| looks_like_rootfs(path));
            let candidate = candidates
                .next()
                .ok_or("archive does not contain a recognizable Linux root filesystem")?;
            if candidates.next().is_some() {
                return Err("archive contains multiple possible root filesystems".into());
            }
            candidate
        };

        fs::rename(&root, &new_rootfs)?;
        if staging.exists() {
            fs::remove_dir_all(&staging)?;
        }
        validate_rootfs(&new_rootfs)?;

        fs::rename(&destination, &backup)?;
        backup_created = true;
        if let Err(error) = fs::rename(&new_rootfs, &destination) {
            let _ = fs::rename(&backup, &destination);
            backup_created = false;
            return Err(error.into());
        }

        let canonical = validate_rootfs(&destination)?;
        let updated = catalog
            .images
            .iter_mut()
            .find(|image| image.name == name)
            .ok_or("image catalog changed during update")?;
        updated.architecture = architecture.to_owned();
        updated.format = "directory".to_owned();
        updated.source = Some(canonical.to_string_lossy().into_owned());

        if let Err(error) = save_catalog(&catalog) {
            let _ = fs::remove_dir_all(&destination);
            let _ = fs::rename(&backup, &destination);
            backup_created = false;
            return Err(error);
        }

        let _ = fs::remove_dir_all(&backup);
        backup_created = false;
        Ok(canonical)
    })();

    if result.is_err() {
        if staging.exists() {
            let _ = fs::remove_dir_all(&staging);
        }
        if new_rootfs.exists() {
            let _ = fs::remove_dir_all(&new_rootfs);
        }
        if backup_created && backup.exists() && !destination.exists() {
            let _ = fs::rename(&backup, &destination);
        }
    }
    result
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

    #[test]
    fn architecture_identifiers_are_restricted() {
        assert!(valid_architecture("aarch64"));
        assert!(valid_architecture("x86_64"));
        assert!(!valid_architecture("../arm64"));
    }
}
