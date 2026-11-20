use clap::{Parser, Subcommand};

use crate::core::{config, elf, host, images, logging, registry, runtime};
use crate::ui;

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
    }
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
        #[arg(long, default_value_t = std::env::consts::ARCH)]
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
        #[arg(long, default_value_t = std::env::consts::ARCH)]
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
            ui::key("Host", format!("{} ({})", host::os_name(), host::architecture()));
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
            ui::state("ELF64 inspector", true, "metadata only");
            ui::state("ELF load planner", true, "mapping plan only");
            ui::state(
                "Native chroot runner",
                runtime::supported(),
                "host-kernel execution; Linux only",
            );
            ui::state(
                "Runtime privileges",
                runtime::has_required_privileges(),
                "root + chroot/setuid/setgid capabilities",
            );
            ui::state("Native executor", false, "not implemented");
            ui::state("ELF memory loader", false, "not implemented");
            ui::state("Syscall layer", false, "not implemented");
            ui::state("CPU emulator", false, "not implemented");
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
                let _ = logging::record(
                    "info",
                    &format!("image pulled: {name} ({architecture})"),
                );
                ui::good(&format!("Image '{name}' downloaded, verified and imported."));
                ui::key("Root filesystem", rootfs.display());
                ui::key("Architecture", architecture);
            }
            ImageCommands::Update {
                name,
                registry: registry_url,
                architecture,
            } => {
                let rootfs = registry::update(&registry_url, &name, &architecture)?;
                let _ = logging::record(
                    "info",
                    &format!("image updated: {name} ({architecture})"),
                );
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
                let _ = logging::record(
                    "info",
                    &format!("image imported: {name} ({architecture})"),
                );
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
                    let _ = logging::record(
                        "info",
                        &format!("image unregistered: {name}"),
                    );
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
                let plan = crate::core::loader::plan_load(&info, load_bias)?;
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
        },
        Commands::Engine { command } => match command {
            EngineCommands::Status { json } => {
                if json {
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
                            {"name": "elf64_inspector", "status": "available"},
                            {"name": "elf_load_planner", "status": "available"},
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
                            {"name": "syscall_layer", "status": "not_implemented"},
                            {"name": "cpu_emulator", "status": "not_implemented"}
                        ],
                        "guest_execution": {
                            "available": runtime::supported() && runtime::has_required_privileges(),
                            "backend": "linux_chroot_host_kernel",
                            "same_architecture_only": true,
                            "reason": if !runtime::supported() {
                                "The native chroot backend requires a Linux host."
                            } else if !runtime::has_required_privileges() {
                                concat!(
                                    "Effective UID 0 and CAP_SYS_CHROOT/CAP_SETUID/CAP_SETGID are required; ",
                                    "ordinary unprivileged PRoot sessions cannot start it."
                                )
                            } else {
                                "Native chroot process launch is implemented; this is not a syscall emulator or security sandbox."
                            }
                        }
                    });
                    println!("{}", serde_json::to_string_pretty(&status)?);
                } else {
                    ui::banner("Engine capability status");
                    ui::heading("Available");
                    ui::state("Core", true, "initialized");
                    ui::state("Host detection", true, "platform information");
                    ui::state("Configuration", true, "user settings");
                    ui::state("Image catalog", true, "local metadata");
                    ui::state("Registry client", true, "HTTPS + SHA-256");
                    ui::state("Event logging", true, "local event log");
                    ui::state("ELF64 inspector", true, "metadata only");
                    ui::state("ELF load planner", true, "planning only");
                    ui::state(
                        "Native chroot runner",
                        runtime::supported(),
                        "Linux host kernel",
                    );
                    ui::heading("Under development");
                    ui::state("Native executor", false, "not implemented");
                    ui::state("ELF memory loader", false, "not implemented");
                    ui::state("Syscall layer", false, "not implemented");
                    ui::state("CPU emulator", false, "not implemented");
                    if runtime::supported() && runtime::has_required_privileges() {
                        ui::good("Native same-architecture chroot execution is available.");
                    } else {
                        ui::warn("Native chroot execution needs a Linux host and effective root.");
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
            let program = &command[0];
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
