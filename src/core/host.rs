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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_fields_are_not_empty() {
        assert!(!os_name().is_empty());
        assert!(!architecture().is_empty());
    }
}
