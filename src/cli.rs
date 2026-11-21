use clap::{Parser, Subcommand};

use crate::ui;
use nelroot::core::{config, elf, host, hub, hub_signing, images, logging, registry, runtime};
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
use nelroot::core::{cpu, stack};
#[cfg(target_os = "linux")]
use nelroot::core::{memory, relocation};
#[derive(Debug, Parser)]
#[command(
    name = "nelroot",
    version,
    about = "Independent Linux execution and virtualization engine",
    long_about = "Independent first-party Linux runtime and systems project by AtDev.",
    color = clap::ColorChoice::Auto
)]
pub struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Display NELRoot and host information.
    Info,
    /// Inspect host capabilities and platform limitations.
    Doctor,
    /// Initialize the user environment and configuration.
    Setup,
    /// Create the default user configuration (alias for setup).
    Init,
    /// Show important NELRoot directories.
    Paths,
    /// Show recent NELRoot event log entries.
    Logs {
        /// Number of recent lines to display.
        #[arg(long, default_value_t = 50)]
        lines: usize,
    },
    /// Manage local configuration.
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
    /// Manage operating-system images.
    Image {
        #[command(subcommand)]
        command: ImageCommands,
    },
    /// Inspect ELF64 executable metadata without executing the file.
    Elf {
        #[command(subcommand)]
        command: ElfCommands,
    },
    /// Inspect the independent NELRoot runtime.
    Engine {
        #[command(subcommand)]
        command: EngineCommands,
    },
    /// Execute a host-native program inside a registered image using Linux chroot.
    Run {
        /// Registered image name.
        image: String,
        /// Guest working directory.
        #[arg(long, default_value = "/")]
        cwd: std::path::PathBuf,
        /// Keep host root credentials inside the chroot (unsafe; only for trusted images).
        #[arg(long)]
        keep_root: bool,
        /// Absolute guest executable and its arguments, after --.
        #[arg(required = true, num_args = 1.., trailing_var_arg = true)]
        command: Vec<String>,
    },
    /// Run a host-native program with rootless user and mount namespace isolation.
    #[cfg(target_os = "linux")]
    RunRootless {
        /// Registered image name.
        image: String,
        /// Guest working directory.
        #[arg(long, default_value = "/")]
        cwd: std::path::PathBuf,
        /// Absolute guest executable and its arguments after --.
        #[arg(required = true, num_args = 1.., trailing_var_arg = true)]
        command: Vec<String>,
    },
    /// Open an interactive shell inside a registered image.
    Shell {
        /// Registered image name.
        image: String,
        /// Guest shell executable.
        #[arg(long, default_value = "/bin/sh")]
        shell: String,
        /// Guest working directory.
        #[arg(long, default_value = "/")]
        cwd: std::path::PathBuf,
        /// Keep host root credentials inside the chroot (unsafe; only for trusted images).
        #[arg(long)]
        keep_root: bool,
    },
}

#[derive(Debug, Subcommand)]
enum ConfigCommands {
    /// Print the configuration file path.
    Path,
    /// Display the current configuration.
    Show,
}

#[derive(Debug, Subcommand)]
enum ImageCommands {
    /// Validate a first-party NELRoot Hub image manifest JSON file.
    HubManifestValidate {
        /// Path to the manifest JSON file.
        path: std::path::PathBuf,
    },
    /// Generate an Ed25519 signing key pair (private key is stored locally).
    HubKeygen {
        /// Output path for the private 32-byte key.
        private_key: std::path::PathBuf,
        /// Output path for the public key (hex encoded).
        public_key: std::path::PathBuf,
    },
    /// Sign the exact bytes of a validated Hub manifest.
    HubSign {
        /// Path to the manifest JSON file.
        manifest: std::path::PathBuf,
        /// Path to the private signing key.
        #[arg(long)]
        key: std::path::PathBuf,
        /// Output path for the detached signature JSON.
        #[arg(long)]
        output: std::path::PathBuf,
    },
    /// Verify a detached manifest signature against an explicitly trusted public key.
    HubVerify {
        /// Path to the manifest JSON file.
        manifest: std::path::PathBuf,
        /// Path to the detached signature JSON.
        signature: std::path::PathBuf,
        /// Path to the trusted public key (hex encoded).
        #[arg(long)]
        key: std::path::PathBuf,
    },
    /// Validate a first-party NELRoot Hub catalog JSON file.
    HubCatalogValidate {
        /// Path to the catalog JSON file.
        path: std::path::PathBuf,
    },
    /// Search the live first-party NELRoot Image Hub catalog.
    HubRemoteSearch {
        /// Search text.
        query: String,
        /// HTTPS base URL of the NELRoot Hub.
        #[arg(long)]
        hub: String,
        /// Optional guest architecture filter.
        #[arg(long)]
        architecture: Option<String>,
    },
    /// Download, verify and import an image from the first-party NELRoot Hub.
    HubPull {
        /// Stable Hub image ID, for example atdev/alpine-3.22.
        image_id: String,
        /// HTTPS base URL of the NELRoot Hub.
        #[arg(long)]
        hub: String,
        /// Guest CPU architecture.
        #[arg(long, default_value_t = std::env::consts::ARCH.to_string())]
        architecture: String,
        /// Optional local image name (otherwise derived from ID and architecture).
        #[arg(long)]
        name: Option<String>,
    },
    /// Publish a local rootfs archive and manifest to the first-party NELRoot Hub.
    HubPublish {
        /// Path to a validated schema-v1 manifest JSON file.
        manifest: std::path::PathBuf,
        /// Matching gzip-compressed rootfs archive.
        archive: std::path::PathBuf,
        /// HTTPS base URL of the NELRoot Hub.
        #[arg(long)]
        hub: String,
    },
    /// Update a NELRoot-managed image from the first-party NELRoot Hub.
    HubUpdate {
        /// Stable Hub image ID, for example atdev/alpine-3.22.
        image_id: String,
        /// Existing local image name to replace.
        name: String,
        /// HTTPS base URL of the NELRoot Hub.
        #[arg(long)]
        hub: String,
        /// Guest CPU architecture.
        #[arg(long, default_value_t = std::env::consts::ARCH.to_string())]
        architecture: String,
    },
    /// Search a local copy of the first-party NELRoot Hub catalog.
    HubCatalogSearch {
        /// Path to the catalog JSON file.
        path: std::path::PathBuf,
        /// Search term matched against image metadata and tags.
        query: String,
        /// Optional guest architecture filter.
        #[arg(long)]
        architecture: Option<String>,
    },
    /// Search public Docker Hub repositories.
    HubSearch {
        /// Search text.
        query: String,
        /// Maximum number of repositories (1-100).
        #[arg(long, default_value_t = 20)]
        limit: u16,
    },
    /// List tags published for a Docker Hub repository.
    HubTags {
        /// Repository name, for example alpine or library/ubuntu.
        repository: String,
        /// Maximum number of tags (1-100).
        #[arg(long, default_value_t = 50)]
        limit: u16,
    },
    /// Inspect a Docker Hub tag and select its Linux platform manifest.
    HubInspect {
        /// Repository name, for example alpine or library/ubuntu.
        repository: String,
        /// Tag or manifest digest.
        #[arg(default_value = "latest")]
        reference: String,
        /// Target architecture (x86_64, aarch64, etc.).
        #[arg(long, default_value_t = std::env::consts::ARCH.to_string())]
        architecture: String,
    },
    /// Search a remote HTTPS image registry index.
    Search {
        /// Search term matched against image names and descriptions.
        query: String,
        /// HTTPS URL of the registry index.json.
        #[arg(long)]
        registry: String,
    },
    /// Download, verify and import an image from a registry.
    Pull {
        /// Image name in the registry.
        name: String,
        /// HTTPS URL of the registry index.json.
        #[arg(long)]
        registry: String,
        /// Guest CPU architecture.
        #[arg(long, default_value_t = std::env::consts::ARCH.to_string())]
        architecture: String,
    },
    /// Update a NELRoot-managed image from a registry.
    Update {
        /// Existing image name in the local catalog and registry.
        name: String,
        /// HTTPS URL of the registry index.json.
        #[arg(long)]
        registry: String,
        /// Guest CPU architecture.
        #[arg(long, default_value_t = std::env::consts::ARCH.to_string())]
        architecture: String,
    },
    /// Download an HTTPS image archive and verify its SHA-256 checksum.
    Download {
        /// HTTPS URL of a .tar.gz image archive.
        url: String,
        /// Local path where the archive will be saved.
        output: std::path::PathBuf,
        /// Expected SHA-256 checksum (64 hexadecimal characters).
        #[arg(long)]
        sha256: String,
    },
    /// Import and safely extract a .tar.gz Linux root filesystem archive.
    Import {
        /// Name to assign to the imported image.
        name: String,
        /// Path to a previously downloaded .tar.gz archive.
        archive: std::path::PathBuf,
        /// CPU architecture of the root filesystem.
        #[arg(long, default_value = "aarch64")]
        architecture: String,
    },
    /// List images registered in the local image catalog.
    List,
    /// Inspect an image registered in the local catalog.
    Inspect {
        /// Image name from the local catalog.
        name: String,
    },
    /// Validate an existing Linux root filesystem directory.
    Validate {
        /// Path to the root filesystem directory.
        path: std::path::PathBuf,
    },
    /// Register an existing Linux root filesystem directory (does not copy it).
    Register {
        /// Name to assign to this image.
        name: String,
        /// Path to an existing Linux root filesystem directory.
        path: std::path::PathBuf,
        /// CPU architecture of the root filesystem.
        #[arg(long, default_value = "aarch64")]
        architecture: String,
    },
    /// Remove an image from the catalog without deleting its files.
    Remove {
        /// Image name to unregister.
        name: String,
    },
    /// Remove stale image staging directories and cached downloads.
    Clean,
}

#[derive(Debug, Subcommand)]
enum ElfCommands {
    /// Validate and display an ELF64 file's header and program headers.
    Inspect {
        /// Path to the ELF file.
        path: std::path::PathBuf,
    },
    /// Build a non-executing memory mapping plan for an ELF64 image.
    Plan {
        /// Path to the ELF file.
        path: std::path::PathBuf,
        /// Page-aligned load bias (primarily for ET_DYN images).
        #[arg(long, default_value_t = 0)]
        load_bias: u64,
    },
    /// Map a static host-native ET_DYN image into memory without executing it (Linux only).
    #[cfg(target_os = "linux")]
    Map {
        /// Path to the ELF file.
        path: std::path::PathBuf,
    },
    /// Map a static PIE and apply supported RELATIVE relocations without executing it (Linux only).
    #[cfg(target_os = "linux")]
    Relocate {
        /// Path to the ELF file.
        path: std::path::PathBuf,
    },
    /// Interpret a static x86_64 PIE with the bounded experimental guest CPU.
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    Emulate {
        /// Path to a static ET_DYN ELF without PT_INTERP.
        path: std::path::PathBuf,
        /// Maximum guest instructions to interpret.
        #[arg(long, default_value_t = 100_000)]
        max_steps: u64,
        /// Additional guest argv entries, after --.
        #[arg(trailing_var_arg = true)]
        arguments: Vec<String>,
    },
}

#[derive(Debug, Subcommand)]
enum EngineCommands {
    /// Show implementation status of engine components.
    Status {
        /// Emit a versioned JSON document for integrations such as NEXORA.
        #[arg(long)]
        json: bool,
    },
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Info => {
            ui::banner("Independent Linux execution & virtualization engine");
            ui::key("Version", env!("CARGO_PKG_VERSION"));
            ui::key("Project", "AtDev");
            ui::key(
                "Host",
                format!("{} ({})", host::os_name(), host::architecture()),
            );
            if let Some(kernel) = host::kernel_release() {
                ui::key("Kernel", kernel);
            }
            ui::muted("Run 'nelroot doctor' to inspect host capabilities.");
        }
        Commands::Doctor => {
            ui::banner("Host diagnostics");
            ui::heading("System");
            ui::key("Operating system", host::os_name());
            ui::key("Architecture", host::architecture());
            ui::key("Pointer width", format!("{}-bit", usize::BITS));
            let privilege_state = match host::has_elevated_privileges() {
                Some(true) => "yes",
                Some(false) => "no",
                None => "unknown",
            };
            ui::key("Elevated process", privilege_state);
            ui::key("Data directory", config::data_dir().display());
            if let Some(kernel) = host::kernel_release() {
                ui::key("Kernel release", kernel);
            }
            ui::heading("Host interfaces");
            for (name, available) in host::host_interfaces() {
                if available {
                    ui::good(&format!("{name}: available"));
                } else {
                    ui::warn(&format!("{name}: unavailable"));
                }
            }
            ui::heading("NELRoot runtime");
            let namespaces = runtime::rootless_namespace_info();
            let namespace_creation = runtime::probe_user_namespace_creation();
            ui::state(
                "Rootless user namespace policy",
                namespaces.potentially_available(),
                "read-only kernel-policy hints",
            );
            ui::state(
                "User namespace creation",
                namespace_creation.is_ok(),
                &namespace_creation
                    .as_ref()
                    .err()
                    .map(|error| error.to_string())
                    .unwrap_or_else(|| "created successfully in disposable child".to_owned()),
            );
            ui::key(
                "User namespace sysctl",
                namespaces
                    .unprivileged_userns_clone
                    .map(|enabled| if enabled { "enabled" } else { "disabled" })
                    .unwrap_or("not exposed"),
            );
            ui::key(
                "Maximum user namespaces",
                namespaces
                    .max_user_namespaces
                    .map(|limit| limit.to_string())
                    .unwrap_or_else(|| "unknown".to_owned()),
            );
            ui::key(
                "UID/GID maps readable",
                format!(
                    "{}/{}",
                    namespaces.uid_map_readable, namespaces.gid_map_readable
                ),
            );
            ui::key(
                "Namespace interfaces",
                format!(
                    "user={} mount={} pid={} network={}",
                    namespaces.current_user_namespace_visible,
                    namespaces.mount_namespace_visible,
                    namespaces.pid_namespace_visible,
                    namespaces.network_namespace_visible
                ),
            );
            ui::state("ELF64 inspector", true, "metadata only");
            ui::state("ELF load planner", true, "mapping plan only");
            ui::state(
                "ELF memory mapper",
                cfg!(all(
                    target_os = "linux",
                    any(target_arch = "x86_64", target_arch = "aarch64")
                )),
                "static ET_DYN; no execution",
            );
            ui::state(
                "ELF relative relocator",
                cfg!(all(
                    target_os = "linux",
                    any(target_arch = "x86_64", target_arch = "aarch64")
                )),
                "RELATIVE RELA only; no execution",
            );
            ui::state(
                "Native chroot runner",
                runtime::supported(),
                "host-kernel execution; Linux only",
            );
            if runtime::has_required_privileges() {
                ui::good("Required chroot/setuid/setgid capabilities are present.");
            } else {
                ui::warn(
                    "Required chroot/setuid/setgid capabilities are unavailable to this process.",
                );
            }
            ui::state(
                "Independent ELF executor",
                false,
                "experimental subset; not general Linux compatibility",
            );
            ui::state(
                "Static PIE preparation",
                cfg!(all(target_os = "linux", target_arch = "x86_64")),
                "loader → guest memory pipeline; x86_64 interpreter",
            );
            ui::state(
                "Syscall integration",
                true,
                "bounded write/exit ABI connected to guest interpreter",
            );
            ui::state(
                "Guest CPU interpreter",
                cfg!(all(target_os = "linux", target_arch = "x86_64")),
                "bounded x86_64 subset; instruction budget required",
            );
            ui::warn("Host interfaces do not imply guest Linux compatibility.");
        }
        Commands::Setup | Commands::Init => {
            let path = config::initialize()?;
            let _ = logging::record("info", "user environment initialized");
            ui::good("NELRoot environment initialized.");
            ui::key("Configuration", path.display());
            ui::key("Image catalog", images::catalog_path().display());
            ui::key("Image storage", images::image_store_dir().display());
            ui::key("Cache", config::cache_dir().display());
            ui::key("Logs", config::logs_dir().display());
            ui::key("Runtime state", config::runtime_dir().display());
        }
        Commands::Paths => {
            ui::banner("Directories and persistent state");
            ui::key("Data directory", config::data_dir().display());
            ui::key("Configuration", config::config_path().display());
            ui::key("Image catalog", images::catalog_path().display());
            ui::key("Image storage", images::image_store_dir().display());
            ui::key("Cache", config::cache_dir().display());
            ui::key("Logs", config::logs_dir().display());
            ui::key("Runtime state", config::runtime_dir().display());
        }
        Commands::Logs { lines } => {
            for line in logging::tail(lines)? {
                println!("{line}");
            }
        }
        Commands::Config { command } => match command {
            ConfigCommands::Path => println!("{}", config::config_path().display()),
            ConfigCommands::Show => {
                let path = config::config_path();
                if !path.exists() {
                    println!("Configuration not initialized. Run nelroot setup first.");
                } else {
                    print!("{}", std::fs::read_to_string(path)?);
                }
            }
        },
        Commands::Image { command } => match command {
            ImageCommands::HubManifestValidate { path } => {
                let manifest = hub::load_manifest(&path)?;
                ui::banner("NELRoot Hub manifest");
                ui::good("Manifest schema and metadata are valid.");
                ui::key("Image ID", &manifest.image.id);
                ui::key("Name", &manifest.image.name);
                ui::key("Version", &manifest.image.version);
                ui::key(
                    "Distribution",
                    format!("{} {}", manifest.image.distribution, manifest.image.release),
                );
                ui::key("Architecture", &manifest.image.architecture);
                ui::key("Rootfs format", &manifest.image.rootfs.format);
                ui::key("Rootfs size", manifest.image.rootfs.size_bytes);
                ui::key("SHA-256", &manifest.image.rootfs.sha256);
                ui::key("Download URL", &manifest.image.rootfs.download_url);
            }
            ImageCommands::HubKeygen {
                private_key,
                public_key,
            } => {
                let key_id = hub_signing::generate_keypair(&private_key, &public_key)?;
                ui::banner("NELRoot Hub Ed25519 identity");
                ui::good("Signing key pair generated.");
                ui::key("Key ID", key_id);
                ui::key("Private key", private_key.display());
                ui::key("Public key", public_key.display());
                ui::warn(
                    "Keep the private key secret and backed up. Share the public key through an independently trusted channel.",
                );
            }
            ImageCommands::HubSign {
                manifest,
                key,
                output,
            } => {
                let signed = hub_signing::sign_manifest(&manifest, &key, &output)?;
                ui::banner("NELRoot Hub manifest signature");
                ui::good("Manifest signature created.");
                ui::key("Key ID", signed.key_id);
                ui::key("Manifest SHA-256", signed.manifest_sha256);
                ui::key("Signature", output.display());
            }
            ImageCommands::HubVerify {
                manifest,
                signature,
                key,
            } => {
                let verified = hub_signing::verify_manifest(&manifest, &signature, &key)?;
                ui::banner("NELRoot Hub signature verification");
                ui::good("Ed25519 signature is valid for the exact manifest bytes.");
                ui::key("Key ID", verified.key_id);
                ui::key("Manifest SHA-256", verified.manifest_sha256);
                ui::muted(
                    "Trust comes from the public key you explicitly supplied; the signature file is not itself a trust source.",
                );
            }
            ImageCommands::HubCatalogValidate { path } => {
                let catalog = hub::load_catalog(&path)?;
                ui::banner("NELRoot Hub catalog");
                ui::good(&format!(
                    "Catalog is valid: {} image entries.",
                    catalog.images.len()
                ));
                ui::key("Schema version", catalog.schema_version);
                ui::key("Generated at", &catalog.generated_at);
            }
            ImageCommands::HubRemoteSearch {
                query,
                hub: hub_url,
                architecture,
            } => {
                let catalog = hub::fetch_remote_catalog(&hub_url)?;
                let matches = hub::search_catalog(&catalog, &query, architecture.as_deref())?;
                ui::banner(&format!("NELRoot Image Hub: {query}"));
                if matches.is_empty() {
                    ui::warn("No first-party Hub images matched the query.");
                } else {
                    for image in matches {
                        ui::key(
                            &image.id,
                            format!(
                                "{} {} · {} · {} · {}",
                                image.name,
                                image.version,
                                image.distribution,
                                image.architecture,
                                image.description
                            ),
                        );
                    }
                }
                ui::muted(&format!("Catalog generated at {}", catalog.generated_at));
            }
            ImageCommands::HubPull {
                image_id,
                hub: hub_url,
                architecture,
                name,
            } => {
                let (local_name, rootfs) =
                    hub::pull_remote_image(&hub_url, &image_id, &architecture, name.as_deref())?;
                let _ = logging::record(
                    "info",
                    &format!("first-party Hub image pulled: {image_id} as {local_name}"),
                );
                ui::good(&format!(
                    "Hub image '{image_id}' downloaded, verified and imported."
                ));
                ui::key("Local image", local_name);
                ui::key("Root filesystem", rootfs.display());
                ui::key("Architecture", architecture);
            }
            ImageCommands::HubPublish {
                manifest,
                archive,
                hub: hub_url,
            } => {
                let token = std::env::var("NELROOT_HUB_PUBLISH_TOKEN")
                    .map_err(|_| "NELROOT_HUB_PUBLISH_TOKEN is not set")?;
                let manifest_url = hub::publish_image(&hub_url, &manifest, &archive, &token)?;
                let _ = logging::record(
                    "info",
                    &format!("first-party Hub manifest published: {}", manifest.display()),
                );
                ui::good("Image archive uploaded and manifest published to NELRoot Hub.");
                ui::key("Manifest", manifest_url);
            }
            ImageCommands::HubUpdate {
                image_id,
                name,
                hub: hub_url,
                architecture,
            } => {
                let rootfs = hub::update_remote_image(&hub_url, &image_id, &architecture, &name)?;
                let _ = logging::record(
                    "info",
                    &format!("first-party Hub image updated: {image_id} as {name}"),
                );
                ui::good(&format!(
                    "Local image '{name}' updated from Hub image '{image_id}'."
                ));
                ui::key("Root filesystem", rootfs.display());
                ui::key("Architecture", architecture);
            }
            ImageCommands::HubCatalogSearch {
                path,
                query,
                architecture,
            } => {
                let catalog = hub::load_catalog(&path)?;
                let matches = hub::search_catalog(&catalog, &query, architecture.as_deref())?;
                ui::banner(&format!("NELRoot Hub catalog: {query}"));
                if matches.is_empty() {
                    ui::warn("No first-party Hub images matched the query.");
                } else {
                    for image in matches {
                        ui::key(
                            &image.id,
                            format!(
                                "{} {} · {} · {} · {}",
                                image.name,
                                image.version,
                                image.distribution,
                                image.architecture,
                                image.description
                            ),
                        );
                    }
                }
                ui::muted("Local catalog search only; remote Hub transport is a later milestone.");
            }
            ImageCommands::HubSearch { query, limit } => {
                let matches = registry::docker_hub_search(&query, limit)?;
                ui::banner(&format!("Docker Hub repositories: {query}"));
                if matches.is_empty() {
                    ui::warn("No Docker Hub repositories matched the query.");
                } else {
                    for repository in matches {
                        let official = if repository.is_official {
                            "official"
                        } else {
                            "community"
                        };
                        ui::key(
                            &repository.repo_name,
                            format!(
                                "{official} · {} stars · {} pulls · {}",
                                repository.star_count,
                                repository.pull_count,
                                repository.short_description
                            ),
                        );
                    }
                }
            }
            ImageCommands::HubTags { repository, limit } => {
                let tags = registry::docker_hub_tags(&repository, limit)?;
                ui::banner(&format!("Docker Hub tags: {repository}"));
                if tags.is_empty() {
                    ui::warn("No tags were returned.");
                } else {
                    for tag in tags {
                        ui::key(
                            &tag.name,
                            format!("{} bytes · updated {}", tag.full_size, tag.last_updated),
                        );
                    }
                }
            }
            ImageCommands::HubInspect {
                repository,
                reference,
                architecture,
            } => {
                let manifest =
                    registry::docker_hub_inspect(&repository, &reference, &architecture)?;
                ui::banner("Docker Hub image manifest");
                ui::key("Repository", &manifest.repository);
                ui::key("Reference", &manifest.reference);
                ui::key(
                    "Digest",
                    if manifest.digest.is_empty() {
                        "not supplied"
                    } else {
                        &manifest.digest
                    },
                );
                ui::key(
                    "Platform",
                    format!("{}/{}", manifest.os, manifest.architecture),
                );
                ui::key("Media type", &manifest.media_type);
                ui::key("Layers", manifest.layer_count);
                ui::key("Compressed layer bytes", manifest.total_layer_bytes);
                ui::muted("Inspection only: image layers have not been downloaded or extracted.");
            }
            ImageCommands::Search {
                query,
                registry: registry_url,
            } => {
                let matches = registry::search(&registry_url, &query)?;
                ui::banner(&format!("Image registry search: {query}"));
                if matches.is_empty() {
                    ui::warn(&format!("No images matched '{query}'."));
                } else {
                    for image in matches {
                        ui::key(
                            &image.name,
                            format!("{} · {}", image.architecture, image.description),
                        );
                    }
                }
            }
            ImageCommands::Pull {
                name,
                registry: registry_url,
                architecture,
            } => {
                let rootfs = registry::pull(&registry_url, &name, &architecture)?;
                let _ = logging::record("info", &format!("image pulled: {name} ({architecture})"));
                ui::good(&format!(
                    "Image '{name}' downloaded, verified and imported."
                ));
                ui::key("Root filesystem", rootfs.display());
                ui::key("Architecture", architecture);
            }
            ImageCommands::Update {
                name,
                registry: registry_url,
                architecture,
            } => {
                let rootfs = registry::update(&registry_url, &name, &architecture)?;
                let _ = logging::record("info", &format!("image updated: {name} ({architecture})"));
                ui::good(&format!("Image '{name}' updated and verified."));
                ui::key("Root filesystem", rootfs.display());
                ui::key("Architecture", architecture);
            }
            ImageCommands::Download {
                url,
                output,
                sha256,
            } => {
                let digest = images::download_archive(&url, &output, &sha256)?;
                let _ = logging::record(
                    "info",
                    &format!("image archive downloaded: {}", output.display()),
                );
                ui::good("Image archive downloaded and verified.");
                ui::key("File", output.display());
                ui::key("SHA-256", digest);
            }
            ImageCommands::Import {
                name,
                archive,
                architecture,
            } => {
                let rootfs = images::import_archive(&name, &archive, &architecture)?;
                let _ =
                    logging::record("info", &format!("image imported: {name} ({architecture})"));
                ui::good(&format!("Imported image '{name}'."));
                ui::key("Root filesystem", rootfs.display());
                ui::key("Architecture", architecture);
            }
            ImageCommands::Validate { path } => {
                let rootfs = images::validate_rootfs(&path)?;
                ui::good("Root filesystem validation passed.");
                ui::key("Path", rootfs.display());
            }
            ImageCommands::Register {
                name,
                path,
                architecture,
            } => {
                images::register_image(&name, &path, &architecture)?;
                let _ = logging::record(
                    "info",
                    &format!("image registered: {name} ({architecture})"),
                );
                ui::good(&format!("Registered image '{name}'."));
                ui::key("Root filesystem", path.canonicalize()?.display());
                ui::muted("The source directory was not copied or modified.");
            }
            ImageCommands::Remove { name } => {
                if images::unregister_image(&name)? {
                    let _ = logging::record("info", &format!("image unregistered: {name}"));
                    ui::good(&format!("Unregistered image '{name}'."));
                    ui::muted("Its files were left untouched.");
                } else {
                    return Err(format!("image '{name}' is not registered").into());
                }
            }
            ImageCommands::List => {
                let catalog = images::load_catalog()?;
                ui::banner("Local image catalog");
                if catalog.images.is_empty() {
                    ui::warn("No images registered.");
                    ui::muted(
                        "Search a registry with: nelroot image search <term> --registry <https-index-url>",
                    );
                } else {
                    ui::heading(&format!("Registered images ({})", catalog.images.len()));
                    for image in catalog.images {
                        ui::key(
                            &image.name,
                            format!("{} · {}", image.architecture, image.format),
                        );
                    }
                }
            }
            ImageCommands::Inspect { name } => {
                let catalog = images::load_catalog()?;
                match catalog.images.iter().find(|image| image.name == name) {
                    Some(image) => {
                        ui::banner("Image details");
                        ui::key("Image", &image.name);
                        ui::key("Architecture", &image.architecture);
                        ui::key("Format", &image.format);
                        let source = image.source.as_deref().unwrap_or("not specified");
                        ui::key("Root filesystem", source);
                        let exists = std::path::Path::new(source).is_dir();
                        if exists {
                            ui::good("Root filesystem path exists.");
                        } else {
                            ui::warn("Root filesystem path is missing.");
                        }
                    }
                    None => return Err(format!("image '{name}' is not registered").into()),
                }
            }
            ImageCommands::Clean => {
                let removed = registry::clean_temporary()?;
                let _ = logging::record(
                    "info",
                    &format!("image cleanup removed {removed} temporary entries"),
                );
                ui::good(&format!("Removed {removed} stale temporary entries."));
                ui::muted("Registered images and root filesystems were not modified.");
            }
        },
        Commands::Elf { command } => match command {
            ElfCommands::Inspect { path } => {
                let info = elf::inspect_file(&path)?;
                ui::banner("ELF64 inspection");
                ui::key("File", path.display());
                ui::key("Type", info.file_type);
                ui::key(
                    "Machine",
                    format!("{} ({})", info.machine, info.architecture),
                );
                ui::key("Entry point", format!("0x{:016x}", info.entry_point));
                if let Some(interpreter) = &info.interpreter {
                    ui::key("Interpreter", interpreter);
                } else {
                    ui::key("Interpreter", "static / not specified");
                }
                ui::heading(&format!("Program headers ({})", info.program_headers.len()));
                for (index, header) in info.program_headers.iter().enumerate() {
                    println!(
                        "  [{index}] t={} f={:#x} o={:#x} va={:#x} fs={} ms={} a={}",
                        header.kind,
                        header.flags,
                        header.offset,
                        header.virtual_address,
                        header.file_size,
                        header.memory_size,
                        header.alignment
                    );
                }
            }
            ElfCommands::Plan { path, load_bias } => {
                let info = elf::inspect_file(&path)?;
                let plan = nelroot::core::loader::plan_load(&info, load_bias)?;
                ui::banner("ELF load plan");
                ui::warn("Inspection only: no memory mapped and no code executed.");
                ui::key("Architecture", plan.architecture);
                ui::key("Load bias", format!("0x{:016x}", plan.load_bias));
                ui::key("Entry point", format!("0x{:016x}", plan.entry_point));
                ui::heading(&format!("Segments ({})", plan.segments.len()));
                for (index, segment) in plan.segments.iter().enumerate() {
                    let mut permissions = String::new();
                    permissions.push(if segment.readable { 'r' } else { '-' });
                    permissions.push(if segment.writable { 'w' } else { '-' });
                    permissions.push(if segment.executable { 'x' } else { '-' });
                    println!(
                        "  [{index}] {:#018x}-{:#018x} {} file={} memory={} offset={:#x}",
                        segment.virtual_start,
                        segment.virtual_end,
                        permissions,
                        segment.file_size,
                        segment.memory_size,
                        segment.file_offset
                    );
                }
            }
            #[cfg(target_os = "linux")]
            ElfCommands::Map { path } => {
                let mapped = nelroot::core::memory::MappedElf::map_file(&path)?;
                ui::banner("ELF memory mapping");
                ui::good("Image mapped into this process; entry point was NOT executed.");
                ui::key("File", path.display());
                ui::key("Reservation", format!("{:#x} bytes", mapped.length()));
                ui::key("Base address", format!("{:#018x}", mapped.base_address()));
                ui::key("Load bias", format!("{:#018x}", mapped.load_bias()));
                ui::key("Entry address", format!("{:#018x}", mapped.entry_address()));
                ui::key(
                    "Lowest virtual page",
                    format!("{:#018x}", mapped.min_virtual_address()),
                );
                ui::muted("The mapping is released automatically when this command exits.");
            }
            #[cfg(target_os = "linux")]
            ElfCommands::Relocate { path } => {
                let (mut mapped, info) = memory::MappedElf::map_file_with_info(&path)?;
                let count = relocation::apply_relative_relocations(&mut mapped, &info)?;
                ui::banner("ELF relative relocations");
                ui::key("File", path.display());
                ui::key("Relocations applied", count);
                ui::key("Load bias", format!("{:#018x}", mapped.load_bias()));
                ui::key("Entry address", format!("{:#018x}", mapped.entry_address()));
                ui::warn(
                    "Relocations applied in memory only; the ELF entry point was not executed.",
                );
            }
            #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
            ElfCommands::Emulate {
                path,
                max_steps,
                arguments,
            } => {
                use std::io::{Read, Write};
                let executable = path.to_string_lossy().into_owned();
                let mut random = [0_u8; 16];
                std::fs::File::open("/dev/urandom")?.read_exact(&mut random)?;
                let mut argv = vec![executable.as_bytes().to_vec()];
                argv.extend(
                    arguments
                        .iter()
                        .map(|argument| argument.as_bytes().to_vec()),
                );
                let stack_config = stack::GuestStackConfig {
                    stack_top: 0x0000_7fff_ffff_f000,
                    argv,
                    envp: Vec::new(),
                    execfn: executable.as_bytes().to_vec(),
                    random,
                    auxv: Vec::new(),
                };
                let mut guest = cpu::X86_64GuestRuntime::from_static_pie(&path, &stack_config)?;
                ui::banner("Experimental NELRoot guest interpreter");
                ui::key("ELF", path.display());
                ui::key("Entry point", format!("{:#018x}", guest.cpu.registers.rip));
                ui::key(
                    "Guest memory",
                    format!("{} bytes", guest.context.memory.allocated_bytes()),
                );
                ui::key("Instruction budget", max_steps);
                ui::warn(
                    "Experimental static x86_64 subset only; not a complete Linux runtime or security sandbox.",
                );
                let reason = guest.run(max_steps)?;
                let stdout = guest.context.output.stdout();
                let stderr = guest.context.output.stderr();
                let mut host_stdout = std::io::stdout().lock();
                let mut host_stderr = std::io::stderr().lock();
                host_stdout.write_all(stdout)?;
                host_stderr.write_all(stderr)?;
                host_stdout.flush()?;
                host_stderr.flush()?;
                ui::key("Guest stop reason", format!("{reason:?}"));
                ui::key("Instructions interpreted", guest.cpu.instructions_executed);
            }
        },
        Commands::Engine { command } => match command {
            EngineCommands::Status { json } => {
                if json {
                    let namespace_creation = runtime::probe_user_namespace_creation();
                    let status = serde_json::json!({
                        "schema_version": 1,
                        "product": "NELRoot",
                        "version": env!("CARGO_PKG_VERSION"),
                        "host": {"os": host::os_name(), "architecture": host::architecture()},
                        "components": [
                            {"name": "core", "status": "available"},
                            {"name": "host_detection", "status": "available"},
                            {"name": "configuration", "status": "available"},
                            {"name": "image_catalog", "status": "available"},
                            {"name": "registry_client", "status": "available"},
                            {"name": "event_logging", "status": "available"},
                            {"name": "process_supervisor", "status": "available"},
                            {"name": "elf64_inspector", "status": "available"},
                            {"name": "elf_load_planner", "status": "available"},
                            {
                                "name": "elf_memory_mapper",
                                "status": if cfg!(all(
                                    target_os = "linux",
                                    any(target_arch = "x86_64", target_arch = "aarch64")
                                )) {
                                    "available"
                                } else {
                                    "not_implemented"
                                }
                            },
                            {
                                "name": "rootless_user_namespace_probe",
                                "status": if runtime::rootless_namespace_info().potentially_available() {
                                    "kernel_policy_appears_available_backend_not_implemented"
                                } else {
                                    "unavailable_or_unknown"
                                }
                            },
                            {
                                "name": "rootless_user_namespace_creation",
                                "status": if namespace_creation.is_ok() {
                                    "created_in_disposable_child"
                                } else {
                                    "unavailable_or_denied"
                                },
                                "errno": namespace_creation
                                    .as_ref()
                                    .err()
                                    .and_then(std::io::Error::raw_os_error)
                            },
                            {
                                "name": "rootless_process_runtime",
                                "status": if cfg!(target_os = "linux") {
                                    "implemented_not_end_to_end_verified"
                                } else {
                                    "not_implemented"
                                }
                            },
                            {"name": "guest_memory_backing", "status": "model_available_not_execution"},
                            {"name": "guest_syscall_abi", "status": "model_available_connected_to_guest_interpreter"},
                            {
                                "name": "elf_relative_relocator",
                                "status": if cfg!(all(
                                    target_os = "linux",
                                    any(target_arch = "x86_64", target_arch = "aarch64")
                                )) {
                                    "available"
                                } else {
                                    "not_implemented"
                                }
                            },
                            {
                                "name": "native_chroot_runtime",
                                "status": if runtime::supported() {
                                    "available"
                                } else {
                                    "not_implemented"
                                }
                            },
                            {"name": "native_executor", "status": "not_implemented"},
                            {"name": "elf_loader", "status": "not_implemented"},
                            {"name": "syscall_layer", "status": "model_available_connected_to_guest_interpreter"},
                            {"name": "guest_cpu_interpreter", "status": "bounded_x86_64_instruction_subset_available"},
                            {"name": "full_linux_cpu_compatibility", "status": "not_implemented"}
                        ],
                        "rootless_guest_execution": {
                            "implemented": cfg!(target_os = "linux"),
                            "available": false,
                            "backend": "linux_user_mount_namespace_pivot_root",
                            "same_architecture_only": true,
                            "verified": false,
                            "reason": "The rootless launcher is implemented, but this status command does not perform a complete image launch. Run nelroot run-rootless to verify this host and image."
                        },
                        "guest_execution": {
                            "available": runtime::supported() && runtime::has_required_privileges(),
                            "backend": "linux_chroot_host_kernel",
                            "same_architecture_only": true,
                            "reason": if !runtime::supported() {
                                "The native chroot backend requires a Linux host."
                            } else if !runtime::has_required_privileges() {
                                concat!(
                                    "Effective UID 0 plus effective CAP_SYS_CHROOT/CAP_SETUID/CAP_SETGID ",
                                    "are required; ",
                                    "ordinary unprivileged PRoot sessions cannot start it."
                                )
                            } else {
                                concat!(
                                    "Native chroot process launch is implemented; this is not a syscall ",
                                    "emulator or security sandbox."
                                )
                            }
                        }
                    });
                    println!("{}", serde_json::to_string_pretty(&status)?);
                } else {
                    ui::banner("Engine capability status");
                    ui::heading("Available");
                    let namespaces = runtime::rootless_namespace_info();
                    let namespace_creation = runtime::probe_user_namespace_creation();
                    ui::state(
                        "Rootless user namespace prerequisites",
                        namespaces.potentially_available(),
                        "read-only kernel-policy probe",
                    );
                    ui::state(
                        "User namespace creation",
                        namespace_creation.is_ok(),
                        &namespace_creation
                            .as_ref()
                            .err()
                            .map(|error| error.to_string())
                            .unwrap_or_else(|| "disposable child probe succeeded".to_owned()),
                    );
                    let namespace_stack = runtime::probe_rootless_user_mount_and_pid_namespaces();
                    ui::state(
                        "Rootless user/mount/PID namespaces",
                        namespace_stack.is_ok(),
                        &namespace_stack
                            .as_ref()
                            .err()
                            .map(|error| error.to_string())
                            .unwrap_or_else(|| {
                                "mapping, private mounts and PID 1 probe succeeded".to_owned()
                            }),
                    );
                    ui::state("Core", true, "initialized");
                    ui::state("Host detection", true, "platform information");
                    ui::state("Configuration", true, "user settings");
                    ui::state("Image catalog", true, "local metadata");
                    ui::state(
                        "Registry client",
                        true,
                        "HTTPS + SHA-256 archive index; Docker Hub search/tags and OCI manifest inspection",
                    );
                    ui::state("Event logging", true, "local event log");
                    ui::state(
                        "Process supervisor",
                        true,
                        "owned child wait/terminate lifecycle",
                    );
                    ui::state("Guest memory", true, "bounded byte-backed model");
                    ui::state(
                        "Guest syscall ABI",
                        true,
                        "x86_64 write/exit model; connected to guest interpreter",
                    );
                    ui::state("ELF64 inspector", true, "metadata only");
                    ui::state("ELF load planner", true, "planning only");
                    ui::state(
                        "ELF memory mapper",
                        cfg!(target_os = "linux"),
                        "static ET_DYN; no execution",
                    );
                    ui::state(
                        "Native chroot runner",
                        runtime::supported(),
                        "Linux host kernel",
                    );
                    ui::heading("Under development");
                    ui::state(
                        "Independent ELF executor",
                        false,
                        "experimental subset; not general Linux compatibility",
                    );
                    ui::state(
                        "Static PIE preparation",
                        cfg!(all(target_os = "linux", target_arch = "x86_64")),
                        "loader → guest memory pipeline",
                    );
                    ui::state(
                        "Guest CPU interpreter",
                        cfg!(all(target_os = "linux", target_arch = "x86_64")),
                        "bounded x86_64 instruction subset; instruction budget required",
                    );
                    ui::state("Full Linux CPU compatibility", false, "not implemented");
                    if runtime::supported() && runtime::has_required_privileges() {
                        ui::good("Native same-architecture chroot execution is available.");
                    } else {
                        ui::warn(
                            "Native chroot needs Linux plus effective root/chroot/setuid/setgid capabilities.",
                        );
                    }
                    ui::warn(
                        "This backend is not a security sandbox or an independent syscall emulator.",
                    );
                    ui::muted("Machine interface: nelroot engine status --json");
                }
            }
        },
        Commands::Run {
            image,
            cwd,
            keep_root,
            command,
        } => {
            let program = command.first().ok_or("missing guest executable")?;
            let arguments = &command[1..];
            ui::banner("Native Linux runtime");
            ui::key("Image", &image);
            ui::key("Executable", program);
            if keep_root {
                ui::warn("Running with host root credentials; use only a trusted image.");
            } else {
                ui::good("Child credentials will be dropped to uid/gid 65534.");
            }
            let status = runtime::run_image(&image, program, arguments, &cwd, keep_root)?;
            if !status.success() {
                std::process::exit(status.code().unwrap_or(128));
            }
        }
        #[cfg(target_os = "linux")]
        Commands::RunRootless {
            image,
            cwd,
            command,
        } => {
            let program = command.first().ok_or("missing guest executable")?;
            let arguments = &command[1..];
            ui::banner("Rootless Linux runtime");
            ui::key("Image", &image);
            ui::key("Executable", program);
            ui::good("Host UID/GID remain unprivileged; rootfs is read-only except /tmp.");
            ui::warn("PID/network namespaces and /proc and /dev mounts are not implemented yet.");
            let status = runtime::run_rootless_image(&image, program, arguments, &cwd)?;
            if !status.success() {
                std::process::exit(status.code().unwrap_or(128));
            }
        }
        Commands::Shell {
            image,
            shell,
            cwd,
            keep_root,
        } => {
            ui::banner("Interactive Linux shell");
            ui::key("Image", &image);
            ui::key("Shell", &shell);
            if keep_root {
                ui::warn("Running with host root credentials; use only a trusted image.");
            } else {
                ui::good("Child credentials will be dropped to uid/gid 65534.");
            }
            let status = runtime::run_image(&image, &shell, &[], &cwd, keep_root)?;
            if !status.success() {
                std::process::exit(status.code().unwrap_or(128));
            }
        }
    }

    Ok(())
}
