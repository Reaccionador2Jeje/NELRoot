use std::{
    ffi::CString,
    io,
    path::Path,
    process::{Command, ExitStatus},
};

use crate::core::images;

/// Whether this build contains the native Linux chroot process backend.
pub fn supported() -> bool {
    cfg!(target_os = "linux")
}

/// Whether the current process has the privilege required by chroot(2).
pub fn has_required_privileges() -> bool {
    #[cfg(target_os = "linux")]
    {
        // SAFETY: geteuid has no pointer arguments or side effects.
        unsafe { libc::geteuid() == 0 }
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

fn validate_guest_cwd(path: &Path) -> Result<(), &'static str> {
    if path.as_os_str().is_empty() || !path.is_absolute() {
        return Err("guest working directory must be an absolute path inside the image");
    }
    if path.components().any(|part| matches!(part, std::path::Component::ParentDir)) {
        return Err("guest working directory must not contain '..'");
    }
    Ok(())
}

fn host_architecture_matches(guest: &str) -> bool {
    let host = std::env::consts::ARCH;
    matches!(
        (host, guest),
        ("aarch64", "aarch64" | "arm64")
            | ("x86_64", "x86_64" | "amd64")
            | ("x86", "x86" | "i686")
            | ("arm", "arm" | "armv7")
    )
}

/// Run one host-native program from a registered rootfs using Linux chroot(2).
///
/// The host kernel executes the guest ELF. This is not an independent ELF loader,
/// syscall emulator, or security sandbox. By default the child drops to uid/gid 65534.
pub fn run_image(
    image_name: &str,
    program: &str,
    arguments: &[String],
    guest_cwd: &Path,
    keep_root: bool,
) -> Result<ExitStatus, Box<dyn std::error::Error>> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (image_name, program, arguments, guest_cwd, keep_root);
        return Err("the native chroot runtime is currently supported on Linux hosts only".into());
    }

    #[cfg(target_os = "linux")]
    {
        if !has_required_privileges() {
            return Err(
                "native chroot requires effective root and CAP_SYS_CHROOT; this does not work inside an ordinary unprivileged PRoot session"
                    .into(),
            );
        }

        if !program.starts_with('/') || program.as_bytes().contains(&0) {
            return Err("guest executable must be an absolute path such as /bin/sh".into());
        }
        validate_guest_cwd(guest_cwd)?;

        let catalog = images::load_catalog()?;
        let image = catalog
            .images
            .iter()
            .find(|image| image.name == image_name)
            .ok_or_else(|| format!("image '{image_name}' is not registered"))?;

        if !host_architecture_matches(&image.architecture) {
            return Err(format!(
                "image architecture '{}' does not match host architecture '{}'; cross-architecture execution is not implemented",
                image.architecture,
                std::env::consts::ARCH
            )
            .into());
        }

        let rootfs = image
            .source
            .as_deref()
            .ok_or("registered image has no root filesystem path")?;
        let rootfs = images::validate_rootfs(Path::new(rootfs))?;
        let cwd_c = CString::new(guest_cwd.as_os_str().as_encoded_bytes())?;
        let root_c = CString::new(rootfs.as_os_str().as_encoded_bytes())?;

        let mut command = Command::new(program);
        command
            .args(arguments)
            .env_clear()
            .env("HOME", if keep_root { "/root" } else { "/nonexistent" })
            .env("PATH", "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin")
            .env("USER", if keep_root { "root" } else { "nobody" })
            .env("LOGNAME", if keep_root { "root" } else { "nobody" });

        if let Some(term) = std::env::var_os("TERM") {
            command.env("TERM", term);
        }

        use std::os::unix::process::CommandExt;
        // SAFETY: the pre-exec closure performs only low-level process setup
        // (chroot/chdir/credential changes) before exec. It does not allocate
        // or access shared Rust state.
        unsafe {
            command.pre_exec(move || {
                if libc::chroot(root_c.as_ptr()) != 0 {
                    return Err(io::Error::last_os_error());
                }
                if libc::chdir(c"/".as_ptr()) != 0 {
                    return Err(io::Error::last_os_error());
                }
                if libc::chdir(cwd_c.as_ptr()) != 0 {
                    return Err(io::Error::last_os_error());
                }

                if !keep_root {
                    if libc::setgroups(0, std::ptr::null()) != 0 {
                        return Err(io::Error::last_os_error());
                    }
                    if libc::setresgid(65534, 65534, 65534) != 0 {
                        return Err(io::Error::last_os_error());
                    }
                    if libc::setresuid(65534, 65534, 65534) != 0 {
                        return Err(io::Error::last_os_error());
                    }
                    if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 {
                        return Err(io::Error::last_os_error());
                    }
                }
                Ok(())
            });
        }

        eprintln!(
            "NELRoot: starting '{}' from {} (host kernel, native architecture)",
            program,
            rootfs.display()
        );
        if keep_root {
            eprintln!("WARNING: guest process retains host root privileges; chroot is not a security boundary.");
        } else {
            eprintln!("NELRoot: guest credentials dropped to uid/gid 65534 (nobody).");
        }

        let status = command.status()?;
        Ok(status)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn architecture_aliases_are_explicit() {
        assert!(host_architecture_matches(match std::env::consts::ARCH {
            "aarch64" => "arm64",
            "x86_64" => "amd64",
            other => other,
        }));
        assert!(!host_architecture_matches("mips64"));
    }

    #[test]
    fn runtime_support_matches_linux_target() {
        assert_eq!(supported(), cfg!(target_os = "linux"));
    }

    #[test]
    fn cwd_validation_requires_absolute_confined_path() {
        assert!(validate_guest_cwd(Path::new("/workspace")).is_ok());
        assert!(validate_guest_cwd(Path::new("workspace")).is_err());
        assert!(validate_guest_cwd(Path::new("/tmp/../etc")).is_err());
    }
}
