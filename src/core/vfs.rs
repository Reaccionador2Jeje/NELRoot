//! Root-confined guest filesystem access.
//!
//! Linux file operations use openat2(2) relative to a pinned root directory
//! descriptor. RESOLVE_IN_ROOT prevents absolute paths, "..", and symlinks
//! from escaping that root, while RESOLVE_NO_MAGICLINKS rejects procfs-style
//! magic links. The older path-resolution helpers remain useful for
//! diagnostics, but must not be used as a security boundary for later opens.

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

    /// Open an existing guest file without following paths outside the root.
    ///
    /// This operation requires Linux 5.6+ with openat2(2). Symlinks that
    /// resolve within the root are allowed; absolute symlinks are interpreted
    /// relative to the guest root by RESOLVE_IN_ROOT.
    #[cfg(target_os = "linux")]
    pub fn open_existing(&self, guest_path: &Path) -> Result<std::fs::File, std::io::Error> {
        self.openat2(guest_path, libc::O_RDONLY | libc::O_CLOEXEC, 0)
    }

    /// Create a new guest file exclusively, without escaping the root.
    ///
    /// The parent directory must already exist. Existing files and symlinks
    /// are never overwritten. New files are created with mode 0600.
    #[cfg(target_os = "linux")]
    pub fn create_new_file(&self, guest_path: &Path) -> Result<std::fs::File, std::io::Error> {
        self.openat2(
            guest_path,
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_CLOEXEC,
            0o600,
        )
    }

    /// Perform an openat2 operation relative to the pinned guest root.
    #[cfg(target_os = "linux")]
    fn openat2(
        &self,
        guest_path: &Path,
        flags: libc::c_int,
        mode: libc::mode_t,
    ) -> Result<std::fs::File, std::io::Error> {
        use std::{
            ffi::CString,
            os::{fd::FromRawFd, unix::ffi::OsStrExt},
        };

        if !guest_path.is_absolute() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "guest paths must be absolute",
            ));
        }
        let path = CString::new(guest_path.as_os_str().as_bytes()).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "guest path contains NUL")
        })?;
        let root = CString::new(self.root.as_os_str().as_bytes()).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "root path contains NUL")
        })?;

        // SAFETY: open receives a valid NUL-terminated path and flags only.
        let root_fd = unsafe {
            libc::open(
                root.as_ptr(),
                libc::O_PATH | libc::O_DIRECTORY | libc::O_CLOEXEC,
            )
        };
        if root_fd < 0 {
            return Err(std::io::Error::last_os_error());
        }

        #[repr(C)]
        struct OpenHow {
            flags: u64,
            mode: u64,
            resolve: u64,
        }

        const RESOLVE_NO_MAGICLINKS: u64 = 0x02;
        const RESOLVE_IN_ROOT: u64 = 0x10;
        let how = OpenHow {
            flags: flags as u64,
            mode: mode as u64,
            resolve: RESOLVE_IN_ROOT | RESOLVE_NO_MAGICLINKS,
        };

        // SAFETY: root_fd is an open directory descriptor; path and how point
        // to valid memory for the duration of the syscall. The kernel copies
        // the open_how structure and path before returning.
        let fd = unsafe {
            libc::syscall(
                libc::SYS_openat2,
                root_fd,
                path.as_ptr(),
                &how as *const OpenHow,
                std::mem::size_of::<OpenHow>(),
            ) as libc::c_int
        };
        let error = if fd < 0 {
            Some(std::io::Error::last_os_error())
        } else {
            None
        };
        // SAFETY: root_fd was returned by open and is owned by this function.
        unsafe { libc::close(root_fd) };

        if let Some(error) = error {
            return Err(error);
        }
        // SAFETY: fd is a newly returned, uniquely owned descriptor.
        Ok(unsafe { std::fs::File::from_raw_fd(fd) })
    }

    /// Return the canonical host path of an existing absolute guest path.
    ///
    /// This is for inspection only. Use open_existing for race-resistant file
    /// access rather than opening the returned path later.
    pub fn resolve_existing(
        &self,
        guest_path: &Path,
    ) -> Result<PathBuf, Box<dyn std::error::Error>> {
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
    /// This is for inspection only and is not atomic with a later create.
    /// Use create_new_file for race-resistant creation on Linux.
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
        match std::fs::symlink_metadata(&destination) {
            Ok(_) => return Err("guest destination already exists".into()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
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
        let root = std::env::temp_dir().join(format!("nelroot-vfs-{}-{nonce}", std::process::id()));
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
        let destination = vfs
            .resolve_new_file(Path::new("/etc/nelroot.conf"))
            .unwrap();
        assert_eq!(destination, root.join("etc/nelroot.conf"));
        assert!(vfs.resolve_new_file(Path::new("/missing/file")).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn openat2_reads_guest_file_and_confines_symlinks() {
        use std::os::unix::fs::symlink;

        let root = test_root();
        let outside = root.with_extension("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("secret"), "outside").unwrap();
        symlink(outside.join("secret"), root.join("etc/escape")).unwrap();

        let vfs = GuestVfs::new(&root).unwrap();
        let mut file = vfs.open_existing(Path::new("/etc/os-release")).unwrap();
        let mut contents = String::new();
        use std::io::Read;
        file.read_to_string(&mut contents).unwrap();
        assert_eq!(contents, "NAME=Test\n");
        assert!(vfs.open_existing(Path::new("/etc/escape")).is_err());

        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn openat2_creates_new_file_without_overwriting() {
        let root = test_root();
        let vfs = GuestVfs::new(&root).unwrap();
        let mut file = vfs.create_new_file(Path::new("/etc/new.conf")).unwrap();
        use std::io::Write;
        file.write_all(b"enabled=true\n").unwrap();
        assert!(vfs.create_new_file(Path::new("/etc/new.conf")).is_err());
        assert_eq!(
            fs::read_to_string(root.join("etc/new.conf")).unwrap(),
            "enabled=true\n"
        );
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
        symlink(outside.join("missing"), root.join("etc/dangling")).unwrap();

        let vfs = GuestVfs::new(&root).unwrap();
        assert!(vfs.resolve_existing(Path::new("/escape/secret")).is_err());
        assert!(vfs.resolve_new_file(Path::new("/escape/new-file")).is_err());
        assert!(vfs.resolve_new_file(Path::new("/etc/dangling")).is_err());

        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }
}
