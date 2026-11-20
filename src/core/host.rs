use std::{fs, path::Path};

/// Return a stable, human-readable host operating-system name.
pub fn os_name() -> &'static str {
    match std::env::consts::OS {
        "android" => "Android",
        "linux" => "Linux",
        "windows" => "Windows",
        "macos" => "macOS",
        other => other,
    }
}

/// Return the host CPU architecture reported by the Rust target.
pub fn architecture() -> &'static str {
    std::env::consts::ARCH
}

/// Read the host kernel release when the platform exposes procfs.
pub fn kernel_release() -> Option<String> {
    fs::read_to_string("/proc/sys/kernel/osrelease")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

/// Whether the current process appears to have administrative privileges.
pub fn has_elevated_privileges() -> Option<bool> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|status| {
                status.lines().find_map(|line| {
                    line.strip_prefix("Uid:")
                        .and_then(|value| value.split_whitespace().nth(1))
                        .and_then(|uid| uid.parse::<u32>().ok())
                })
            })
            .map(|uid| uid == 0)
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        None
    }
}

/// Check for common host interfaces without assuming they imply guest compatibility.
pub fn host_interfaces() -> Vec<(&'static str, bool)> {
    vec![
        ("procfs", Path::new("/proc").is_dir()),
        ("sysfs", Path::new("/sys").is_dir()),
        ("device filesystem", Path::new("/dev").is_dir()),
        ("kernel release metadata", kernel_release().is_some()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_fields_are_not_empty() {
        assert!(!os_name().is_empty());
        assert!(!architecture().is_empty());
    }

    #[test]
    fn interface_names_are_stable() {
        assert_eq!(host_interfaces().len(), 4);
    }
}
