use std::{path::Path, process::ExitStatus};

#[cfg(target_os = "linux")]
use std::{
    ffi::CString,
    io,
    process::Command,
};

#[cfg(target_os = "linux")]
use crate::core::{images, process::SupervisedProcess};

/// Read-only diagnostics for Linux user-namespace prerequisites. This is a
/// capability hint, not proof that namespace creation will pass seccomp or LSM policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RootlessNamespaceInfo {
    pub current_user_namespace_visible: bool,
    pub uid_map_readable: bool,
    pub gid_map_readable: bool,
    pub mount_namespace_visible: bool,
    pub pid_namespace_visible: bool,
    pub network_namespace_visible: bool,
    pub unprivileged_userns_clone: Option<bool>,
    pub max_user_namespaces: Option<u64>,
}

impl RootlessNamespaceInfo {
    /// Whether kernel settings appear to permit an unprivileged user namespace.
    pub fn potentially_available(self) -> bool {
        self.current_user_namespace_visible
            && self.max_user_namespaces.is_none_or(|limit| limit > 0)
            && self.unprivileged_userns_clone != Some(false)
    }
}

fn parse_sysctl_bool(value: &str) -> Option<bool> {
    match value.trim() {
        "0" => Some(false),
        "1" => Some(true),
        _ => None,
    }
}

fn parse_namespace_limit(value: &str) -> Option<u64> {
    value.trim().parse().ok()
}

/// Inspect user-namespace policy without changing host state or requiring root.
pub fn rootless_namespace_info() -> RootlessNamespaceInfo {
    #[cfg(target_os = "linux")]
    {
        let current_user_namespace_visible = Path::new("/proc/self/ns/user").exists();
        let uid_map_readable = std::fs::read_to_string("/proc/self/uid_map").is_ok();
        let gid_map_readable = std::fs::read_to_string("/proc/self/gid_map").is_ok();
        let mount_namespace_visible = Path::new("/proc/self/ns/mnt").exists();
        let pid_namespace_visible = Path::new("/proc/self/ns/pid").exists();
        let network_namespace_visible = Path::new("/proc/self/ns/net").exists();
        let unprivileged_userns_clone =
            std::fs::read_to_string("/proc/sys/kernel/unprivileged_userns_clone")
                .ok()
                .and_then(|value| parse_sysctl_bool(&value));
        let max_user_namespaces =
            std::fs::read_to_string("/proc/sys/user/max_user_namespaces")
                .ok()
                .and_then(|value| parse_namespace_limit(&value));

        RootlessNamespaceInfo {
            current_user_namespace_visible,
            uid_map_readable,
            gid_map_readable,
            mount_namespace_visible,
            pid_namespace_visible,
            network_namespace_visible,
            unprivileged_userns_clone,
            max_user_namespaces,
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        RootlessNamespaceInfo {
            current_user_namespace_visible: false,
            uid_map_readable: false,
            gid_map_readable: false,
            mount_namespace_visible: false,
            pid_namespace_visible: false,
            network_namespace_visible: false,
            unprivileged_userns_clone: None,
            max_user_namespaces: None,
        }
    }
}

/// Attempt user-namespace creation in a disposable child process.
///
/// The parent process is never moved into a namespace. The child performs only
/// the unshare syscall, reads errno on failure, and exits immediately. This
/// verifies user-namespace creation only; it does not test UID/GID mapping,
/// mount namespaces, PID namespaces, or rootfs setup.
pub fn probe_user_namespace_creation() -> std::io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        // SAFETY: the child branch below performs only async-signal-safe
        // operations before _exit; it never resumes normal Rust execution.
        let pid = unsafe { libc::fork() };
        if pid < 0 {
            return Err(std::io::Error::last_os_error());
        }
        if pid == 0 {
            // SAFETY: after fork the child uses only unshare, a direct errno
            // read, and _exit; it does not touch inherited Rust runtime state.
            let result = unsafe { libc::unshare(libc::CLONE_NEWUSER) };
            if result == 0 {
                unsafe { libc::_exit(0) };
            }
            let errno = unsafe { *libc::__errno_location() };
            unsafe { libc::_exit(errno.clamp(1, 255)) };
        }

        let mut status = 0;
        loop {
            // SAFETY: status is a valid writable pointer and pid is the child
            // returned by fork; waitpid does not retain the pointer.
            let waited = unsafe { libc::waitpid(pid, &mut status, 0) };
            if waited == pid {
                break;
            }
            if waited < 0 {
                let error = std::io::Error::last_os_error();
                if error.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(error);
            }
        }

        if libc::WIFEXITED(status) {
            let code = libc::WEXITSTATUS(status);
            if code == 0 {
                Ok(())
            } else {
                Err(std::io::Error::from_raw_os_error(code))
            }
        } else {
            Err(std::io::Error::other(
                "user-namespace probe child did not exit normally",
            ))
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "user namespaces are only available on Linux",
        ))
    }
}

/// Exercise user mapping, private mount setup, and PID namespace creation.
///
/// A disposable child enters a user namespace and blocks on a pipe. The parent
/// installs its UID/GID maps, then releases it to create a private mount
/// namespace and PID namespace. A grandchild verifies that it becomes PID 1.
/// No host namespace or mount propagation is changed; no guest rootfs is mounted.
#[cfg(target_os = "linux")]
pub fn probe_rootless_user_mount_and_pid_namespaces() -> io::Result<UserNamespaceMapping> {
    let mut ready_pipe = [0; 2];
    let mut release_pipe = [0; 2];

    // SAFETY: pipe2 initializes the two descriptors in each valid array.
    if unsafe { libc::pipe2(ready_pipe.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        return Err(io::Error::last_os_error());
    }
    if unsafe { libc::pipe2(release_pipe.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        let error = io::Error::last_os_error();
        unsafe {
            libc::close(ready_pipe[0]);
            libc::close(ready_pipe[1]);
        }
        return Err(error);
    }

    // SAFETY: the child performs only close/unshare/read/write/mount/_exit
    // syscalls after fork and never resumes normal Rust execution.
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        let error = io::Error::last_os_error();
        unsafe {
            libc::close(ready_pipe[0]);
            libc::close(ready_pipe[1]);
            libc::close(release_pipe[0]);
            libc::close(release_pipe[1]);
        }
        return Err(error);
    }

    if pid == 0 {
        unsafe {
            libc::close(ready_pipe[0]);
            libc::close(release_pipe[1]);

            let namespace_result = libc::unshare(libc::CLONE_NEWUSER);
            let result_byte = if namespace_result == 0 {
                0_u8
            } else {
                (*libc::__errno_location()).clamp(1, 255) as u8
            };
            if libc::write(ready_pipe[1], (&result_byte as *const u8).cast(), 1) != 1 {
                libc::_exit(248);
            }
            if result_byte != 0 {
                libc::_exit(result_byte as i32);
            }

            let mut release = 0_u8;
            if libc::read(release_pipe[0], (&mut release as *mut u8).cast(), 1) != 1
                || release != 1
            {
                libc::_exit(247);
            }

            if libc::unshare(libc::CLONE_NEWNS) != 0 {
                let errno = *libc::__errno_location();
                libc::_exit(errno.clamp(1, 255));
            }
            if libc::mount(
                std::ptr::null(),
                c"/".as_ptr(),
                std::ptr::null(),
                (libc::MS_REC | libc::MS_PRIVATE) as libc::c_ulong,
                std::ptr::null(),
            ) != 0
            {
                let errno = *libc::__errno_location();
                libc::_exit(errno.clamp(1, 255));
            }
            if libc::unshare(libc::CLONE_NEWPID) != 0 {
                let errno = *libc::__errno_location();
                libc::_exit(errno.clamp(1, 255));
            }
            let init_pid = libc::fork();
            if init_pid < 0 {
                let errno = *libc::__errno_location();
                libc::_exit(errno.clamp(1, 255));
            }
            if init_pid == 0 {
                libc::_exit(if libc::getpid() == 1 { 0 } else { 246 });
            }

            let mut init_status = 0;
            loop {
                let waited = libc::waitpid(init_pid, &mut init_status, 0);
                if waited == init_pid {
                    break;
                }
                if waited < 0 {
                    let errno = *libc::__errno_location();
                    if errno == libc::EINTR {
                        continue;
                    }
                    libc::_exit(errno.clamp(1, 255));
                }
            }
            if !libc::WIFEXITED(init_status) || libc::WEXITSTATUS(init_status) != 0 {
                libc::_exit(245);
            }
            libc::_exit(0);
        }
    }

    // Parent owns the opposite ends of both synchronization pipes.
    unsafe {
        libc::close(ready_pipe[1]);
        libc::close(release_pipe[0]);
    }

    let mut ready = 0_u8;
    // SAFETY: ready points to one writable byte and the pipe descriptor is open.
    let read_result = unsafe { libc::read(ready_pipe[0], (&mut ready as *mut u8).cast(), 1) };
    unsafe { libc::close(ready_pipe[0]) };
    if read_result != 1 || ready != 0 {
        unsafe {
            libc::close(release_pipe[1]);
            libc::kill(pid, libc::SIGKILL);
        }
        let status = wait_for_child(pid)?;
        let _ = status;
        return Err(if read_result == 1 {
            io::Error::from_raw_os_error(ready as i32)
        } else {
            io::Error::last_os_error()
        });
    }

    let mapping = match configure_user_namespace_mappings(pid) {
        Ok(mapping) => mapping,
        Err(error) => {
            unsafe {
                libc::close(release_pipe[1]);
                libc::kill(pid, libc::SIGKILL);
            }
            let _ = wait_for_child(pid);
            return Err(error);
        }
    };

    let release = 1_u8;
    // SAFETY: release points to one readable byte for the waiting child.
    let write_result =
        unsafe { libc::write(release_pipe[1], (&release as *const u8).cast(), 1) };
    unsafe { libc::close(release_pipe[1]) };
    if write_result != 1 {
        unsafe { libc::kill(pid, libc::SIGKILL) };
        let _ = wait_for_child(pid);
        return Err(io::Error::last_os_error());
    }

    let status = wait_for_child(pid)?;
    if libc::WIFEXITED(status) && libc::WEXITSTATUS(status) == 0 {
        Ok(mapping)
    } else if libc::WIFEXITED(status) {
        Err(io::Error::from_raw_os_error(libc::WEXITSTATUS(status)))
    } else {
        Err(io::Error::other(
            "rootless namespace probe child did not exit normally",
        ))
    }
}

#[cfg(target_os = "linux")]
fn wait_for_child(pid: libc::pid_t) -> io::Result<libc::c_int> {
    let mut status = 0;
    loop {
        // SAFETY: status is a valid writable pointer and pid is our child.
        let waited = unsafe { libc::waitpid(pid, &mut status, 0) };
        if waited == pid {
            return Ok(status);
        }
        if waited < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
    }
}

/// The single-ID mapping installed for a rootless user namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UserNamespaceMapping {
    /// Namespace UID 0 maps to this unprivileged host UID.
    pub host_uid: u32,
    /// Namespace GID 0 maps to this unprivileged host GID.
    pub host_gid: u32,
}

#[cfg(target_os = "linux")]
fn format_id_map(host_id: u32) -> String {
    format!("0 {host_id} 1\n")
}

/// Configure a paused child that has already entered a new user namespace.
///
/// The caller must keep the child blocked until this function returns, then
/// release it to continue. This helper writes only the child's procfs mapping
/// files; it does not create namespaces, synchronize a child, or launch a guest.
#[cfg(target_os = "linux")]
pub fn configure_user_namespace_mappings(
    child_pid: libc::pid_t,
) -> io::Result<UserNamespaceMapping> {
    if child_pid <= 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "child PID must be a positive process ID",
        ));
    }

    // SAFETY: geteuid/getegid take no pointers and only read process credentials.
    let mapping = UserNamespaceMapping {
        host_uid: unsafe { libc::geteuid() },
        host_gid: unsafe { libc::getegid() },
    };
    let proc_child = Path::new("/proc").join(child_pid.to_string());

    // The kernel requires setgroups to be disabled before an unprivileged
    // process writes gid_map. Fail closed if this operation is unavailable.
    std::fs::write(proc_child.join("setgroups"), "deny\n")?;
    std::fs::write(proc_child.join("uid_map"), format_id_map(mapping.host_uid))?;
    std::fs::write(proc_child.join("gid_map"), format_id_map(mapping.host_gid))?;

    Ok(mapping)
}

/// Create a private mount namespace after user-namespace mappings are installed.
///
/// Call this inside the child after it has entered a user namespace and received
/// valid UID/GID maps. It creates a new mount namespace and makes propagation
/// recursively private. It does not mount a rootfs or launch a guest process.
#[cfg(target_os = "linux")]
pub fn prepare_private_mount_namespace() -> io::Result<()> {
    // SAFETY: unshare changes only the calling process's namespace membership.
    if unsafe { libc::unshare(libc::CLONE_NEWNS) } != 0 {
        return Err(io::Error::last_os_error());
    }

    // SAFETY: null source/filesystem/data are valid for a propagation change;
    // "/" is the mount target and MS_REC|MS_PRIVATE are mount flags.
    let result = unsafe {
        libc::mount(
            std::ptr::null(),
            c"/".as_ptr(),
            std::ptr::null(),
            (libc::MS_REC | libc::MS_PRIVATE) as libc::c_ulong,
            std::ptr::null(),
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }

    Ok(())
}

/// Whether this build contains the native Linux chroot process backend.
pub fn supported() -> bool {
    cfg!(target_os = "linux")
}

/// Whether the current process has the privilege required by chroot(2).
pub fn has_required_privileges() -> bool {
    #[cfg(target_os = "linux")]
    {
        // SAFETY: geteuid has no pointer arguments or side effects.
        let is_root = unsafe { libc::geteuid() == 0 };
        let has_chroot_capability = std::fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|status| {
                status.lines().find_map(|line| {
                    line.strip_prefix("CapEff:")
                        .and_then(|value| u64::from_str_radix(value.trim(), 16).ok())
                })
            })
            .is_some_and(|effective| {
                let required = (1_u64 << 18) | (1_u64 << 7) | (1_u64 << 6);
                effective & required == required
            });
        is_root && has_chroot_capability
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

#[cfg(any(target_os = "linux", test))]
fn validate_guest_cwd(path: &Path) -> Result<(), &'static str> {
    if path.as_os_str().is_empty() || !path.is_absolute() {
        return Err("guest working directory must be an absolute path inside the image");
    }
    if path
        .components()
        .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err("guest working directory must not contain '..'");
    }
    Ok(())
}

#[cfg(any(target_os = "linux", test))]
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
        Err("the native chroot runtime is currently supported on Linux hosts only".into())
    }

    #[cfg(target_os = "linux")]
    {
        if !has_required_privileges() {
            return Err(
                "native chroot requires effective UID 0 plus CAP_SYS_CHROOT, CAP_SETUID and CAP_SETGID; ordinary unprivileged PRoot sessions do not provide them"
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
            eprintln!(
                "WARNING: guest process retains host root privileges; chroot is not a security boundary."
            );
        } else {
            eprintln!("NELRoot: guest credentials dropped to uid/gid 65534 (nobody).");
        }

        let mut process = SupervisedProcess::spawn(&mut command)?;
        process.wait().map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_user_namespace_sysctl_values() {
        assert_eq!(parse_sysctl_bool("1\n"), Some(true));
        assert_eq!(parse_sysctl_bool("0"), Some(false));
        assert_eq!(parse_sysctl_bool("enabled"), None);
        assert_eq!(parse_namespace_limit(" 42\n"), Some(42));
        assert_eq!(parse_namespace_limit("-1"), None);
    }

    #[test]
    fn rootless_namespace_policy_requires_visible_namespace_and_nonzero_limit() {
        let available = RootlessNamespaceInfo {
            current_user_namespace_visible: true,
            uid_map_readable: true,
            gid_map_readable: true,
            mount_namespace_visible: true,
            pid_namespace_visible: true,
            network_namespace_visible: true,
            unprivileged_userns_clone: Some(true),
            max_user_namespaces: Some(1024),
        };
        assert!(available.potentially_available());
        assert!(
            !RootlessNamespaceInfo {
                max_user_namespaces: Some(0),
                ..available
            }
            .potentially_available()
        );
        assert!(
            !RootlessNamespaceInfo {
                unprivileged_userns_clone: Some(false),
                ..available
            }
            .potentially_available()
        );
        assert!(
            !RootlessNamespaceInfo {
                current_user_namespace_visible: false,
                ..available
            }
            .potentially_available()
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn user_namespace_id_map_format_is_single_id_and_newline_terminated() {
        assert_eq!(format_id_map(1000), "0 1000 1\n");
        assert_eq!(format_id_map(0), "0 0 1\n");
    }

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
