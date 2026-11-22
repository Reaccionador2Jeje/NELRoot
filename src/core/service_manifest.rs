//! Versioned, declarative service manifest parsing and validation.
//!
//! This module defines service intent only. It never starts processes, opens
//! sockets, reads secrets, or claims that a service is running.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ServiceManifest {
    pub schema_version: u32,
    pub services: Vec<ServiceDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ServiceDefinition {
    pub name: String,
    /// Absolute executable path inside the future guest root filesystem.
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default = "default_working_directory")]
    pub working_directory: String,
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
    #[serde(default)]
    pub restart: RestartPolicy,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum RestartPolicy {
    #[default]
    Never,
    OnFailure,
}

fn default_working_directory() -> String {
    "/".to_owned()
}

impl ServiceManifest {
    /// Parse a TOML manifest and validate the complete document.
    pub fn parse_toml(contents: &str) -> Result<Self, String> {
        let manifest: Self = toml::from_str(contents)
            .map_err(|error| format!("invalid service manifest TOML: {error}"))?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Validate schema, identifiers and guest-only paths before any future use.
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 {
            return Err(format!(
                "unsupported service manifest schema version: {}",
                self.schema_version
            ));
        }
        if self.services.is_empty() {
            return Err("service manifest must define at least one service".to_owned());
        }
        if self.services.len() > 128 {
            return Err("service manifest exceeds the 128-service limit".to_owned());
        }

        let mut names = HashSet::new();
        for service in &self.services {
            if !valid_identifier(&service.name) {
                return Err(format!("invalid service name: '{}'", service.name));
            }
            if !names.insert(service.name.as_str()) {
                return Err(format!("duplicate service name: '{}'", service.name));
            }
            if !guest_absolute_path(&service.command) {
                return Err(format!(
                    "service '{}' command must be a normalized absolute guest path",
                    service.name
                ));
            }
            if !guest_absolute_path(&service.working_directory) {
                return Err(format!(
                    "service '{}' working_directory must be a normalized absolute guest path",
                    service.name
                ));
            }
            if service.args.len() > 256 {
                return Err(format!("service '{}' has too many arguments", service.name));
            }
            if service.command.len() > 4096
                || service.args.iter().any(|arg| arg.len() > 4096 || arg.contains('\0'))
            {
                return Err(format!("service '{}' contains an invalid or oversized argument", service.name));
            }
            if service.environment.len() > 128 {
                return Err(format!("service '{}' has too many environment variables", service.name));
            }
            for (key, value) in &service.environment {
                if !valid_environment_key(key)
                    || value.len() > 8192
                    || value.contains('\0')
                    || value.contains('\n')
                    || value.contains('\r')
                {
                    return Err(format!(
                        "service '{}' contains an invalid environment entry",
                        service.name
                    ));
                }
            }
        }
        Ok(())
    }
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
}

fn valid_environment_key(value: &str) -> bool {
    let mut chars = value.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        && value.len() <= 128
}

/// Reject traversal, repeated separators, dot components and non-absolute paths.
fn guest_absolute_path(value: &str) -> bool {
    if value.is_empty() || value.len() > 4096 || !value.starts_with('/') || value.contains('\0') {
        return false;
    }
    if value == "/" {
        return true;
    }
    !value[1..].split('/').any(|component| {
        component.is_empty() || component == "." || component == ".."
    }) && Path::new(value).is_absolute()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_manifest() -> ServiceManifest {
        ServiceManifest {
            schema_version: 1,
            services: vec![ServiceDefinition {
                name: "web".to_owned(),
                command: "/usr/bin/python3".to_owned(),
                args: vec!["-m".to_owned(), "http.server".to_owned()],
                working_directory: "/srv/site".to_owned(),
                environment: BTreeMap::from([("PORT".to_owned(), "8080".to_owned())]),
                restart: RestartPolicy::OnFailure,
            }],
        }
    }

    #[test]
    fn accepts_a_valid_service_manifest() {
        assert!(valid_manifest().validate().is_ok());
    }

    #[test]
    fn rejects_unknown_schema_versions_and_empty_manifests() {
        let mut manifest = valid_manifest();
        manifest.schema_version = 2;
        assert!(manifest.validate().is_err());
        manifest.schema_version = 1;
        manifest.services.clear();
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn rejects_duplicate_names_and_host_path_traversal() {
        let mut manifest = valid_manifest();
        manifest.services.push(manifest.services[0].clone());
        assert!(manifest.validate().is_err());

        let mut manifest = valid_manifest();
        manifest.services[0].working_directory = "/srv/../host".to_owned();
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn rejects_invalid_environment_keys_and_control_characters() {
        let mut manifest = valid_manifest();
        manifest.services[0].environment.insert("BAD-KEY".to_owned(), "x".to_owned());
        assert!(manifest.validate().is_err());

        let mut manifest = valid_manifest();
        manifest.services[0].environment.insert("SAFE".to_owned(), "line\nbreak".to_owned());
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn parses_toml_and_rejects_unknown_fields() {
        let valid = r#"
schema_version = 1

[[services]]
name = "web"
command = "/usr/bin/python3"
args = ["-m", "http.server"]
working_directory = "/srv/site"
restart = "on_failure"
"#;
        assert!(ServiceManifest::parse_toml(valid).is_ok());

        let invalid = r#"
schema_version = 1
unexpected = true

[[services]]
name = "web"
command = "/usr/bin/python3"
"#;
        assert!(ServiceManifest::parse_toml(invalid).is_err());
    }
}
