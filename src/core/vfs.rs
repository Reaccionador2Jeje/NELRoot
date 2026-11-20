//! Root-confined guest filesystem path resolution.
//!
//! This module does not mount filesystems or emulate Linux file descriptors.
//! It provides a safe path-resolution foundation for a future guest VFS.

use std::path::{Component, Path, PathBuf};

/// A filesystem view rooted at one canonical host directory.
#[derive(Debug, Clone)]
pub struct GuestVfs {
    root: PathBuf,
}

impl GuestVfs {
    /// Open an existing root filesystem directory.
    pub fn new(root: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let root = root.canonicalize()?;
        if !root.is_dir() {
            return Err("guest filesystem root must be a directory".into());
        }
        Ok(Self { root })
    }

    /// Return the canonical host path of an existing absolute guest path.
    ///
    /// Symlinks are resolved by the host filesystem and the final canonical
    /// path must remain beneath the configured guest root.
    pub fn resolve_existing(&self, guest_path: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let relative = normalize_guest_path(guest_path)?;
        let candidate = self.root.join(relative);
        let resolved = candidate.canonicalize()?;
        if !resolved.starts_with(&self.root) {
            return Err("guest path resolves outside the configured root filesystem".into());
        }
        Ok(resolved)
    }

    /// Resolve a new file path whose parent directory already exists.
    ///
    /// The final path must not exist. Its canonical parent is checked after
    /// symlink resolution, preventing writes through an escaping parent link.
    pub fn resolve_new_file(
        &self,
        guest_path: &Path,
    ) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let relative = normalize_guest_path(guest_path)?;
        let name = relative
            .file_name()
            .ok_or("guest file path must name a file")?;
        let parent = relative
            .parent()
            .ok_or("guest file path has no parent directory")?;
        let canonical_parent = self.root.join(parent).canonicalize()?;
        if !canonical_parent.starts_with(&self.root) {
            return Err("guest file parent resolves outside the configured root filesystem".into());
        }
        let destination = canonical_parent.join(name);
        if destination.exists() {
            return Err("guest destination already exists".into());
        }
        Ok(destination)
    }

    /// Canonical host path of the configured root filesystem.
    pub fn root(&self) -> &Path {
        &self.root
    }
}

/// Normalize an absolute guest path into a relative path without allowing
/// traversal above the guest root.
fn normalize_guest_path(path: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    if !path.is_absolute() {
        return Err("guest paths must be absolute".into());
    }

    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::RootDir | Component::CurDir => {}
            Component::Normal(part) => normalized.push(part),
            Component::ParentDir => {
                if !normalized.pop() {
                    return Err("guest path attempts to escape its root".into());
                }
            }
            Component::Prefix(_) => {
                return Err("guest path prefixes are unsupported".into());
            }
        }
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, time::SystemTime};

    fn test_root() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "nelroot-vfs-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(root.join("etc")).unwrap();
        fs::write(root.join("etc/os-release"), "NAME=Test\n").unwrap();
        root
    }

    #[test]
    fn resolves_existing_guest_files_under_root() {
        let root = test_root();
        let vfs = GuestVfs::new(&root).unwrap();
        let resolved = vfs.resolve_existing(Path::new("/etc/os-release")).unwrap();
        assert_eq!(resolved, root.join("etc/os-release"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_relative_and_root_escape_paths() {
        assert!(normalize_guest_path(Path::new("etc/passwd")).is_err());
        assert!(normalize_guest_path(Path::new("/../../etc/passwd")).is_err());
        assert_eq!(
            normalize_guest_path(Path::new("/usr/../etc/passwd")).unwrap(),
            PathBuf::from("etc/passwd")
        );
    }

    #[test]
    fn resolves_new_file_only_inside_existing_guest_directory() {
        let root = test_root();
        let vfs = GuestVfs::new(&root).unwrap();
        let destination = vfs.resolve_new_file(Path::new("/etc/nelroot.conf")).unwrap();
        assert_eq!(destination, root.join("etc/nelroot.conf"));
        assert!(vfs.resolve_new_file(Path::new("/missing/file")).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinks_that_escape_guest_root() {
        use std::os::unix::fs::symlink;

        let root = test_root();
        let outside = root.with_extension("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("secret"), "outside").unwrap();
        symlink(&outside, root.join("escape")).unwrap();

        let vfs = GuestVfs::new(&root).unwrap();
        assert!(vfs.resolve_existing(Path::new("/escape/secret")).is_err());
        assert!(vfs.resolve_new_file(Path::new("/escape/new-file")).is_err());

        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }
}
