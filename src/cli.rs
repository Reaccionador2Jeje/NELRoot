use clap::{Parser, Subcommand};

use crate::core::{config, host, images, logging, registry};

#[derive(Debug, Parser)]
#[command(
    name = "nelroot",
    version,
    about = "Independent Linux execution and virtualization engine",
    long_about = "NELRoot is an independent systems project by AtDev. Its runtime, executable loader and CPU emulator are developed as first-party components."
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
    /// Inspect the independent NELRoot runtime.
    Engine {
        #[command(subcommand)]
        command: EngineCommands,
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
        #[arg(long, default_value = std::env::consts::ARCH)]
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
    /// Remove abandoned image extraction staging directories.
    Clean,
}

#[derive(Debug, Subcommand)]
enum EngineCommands {
    /// Show implementation status of engine components.
    Status,
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Info => {
            println!("NELRoot {}", env!("CARGO_PKG_VERSION"));
            println!("Independent Linux execution & virtualization engine");
            println!("Project: AtDev");
            println!("Host: {} ({})", host::os_name(), host::architecture());
            if let Some(kernel) = host::kernel_release() {
                println!("Kernel: {kernel}");
            }
        }
        Commands::Doctor => {
            println!("NELRoot host diagnostics");
            println!("------------------------");
            println!("Operating system : {}", host::os_name());
            println!("Architecture     : {}", host::architecture());
            println!("Pointer width    : {}-bit", usize::BITS);
            println!("Elevated process : {}", host::has_elevated_privileges());
            println!("Data directory   : {}", config::data_dir().display());
            if let Some(kernel) = host::kernel_release() {
                println!("Kernel release   : {kernel}");
            } else {
                println!("Kernel release   : unavailable");
            }
            println!("Host interfaces:");
            for (name, available) in host::host_interfaces() {
                println!("  {:24} {}", name, if available { "available" } else { "unavailable" });
            }
            println!("Execution engine : not implemented yet");
            println!("ELF loader       : not implemented yet");
            println!("Syscall layer    : not implemented yet");
            println!("CPU emulator     : not implemented yet");
            println!("Note             : host interfaces do not imply guest compatibility");
        }
        Commands::Setup | Commands::Init => {
            let path = config::initialize()?;
            let _ = logging::record("info", "user environment initialized");
            println!("NELRoot environment initialized.");
            println!("Configuration: {}", path.display());
            println!("Image catalog: {}", images::catalog_path().display());
            println!("Image storage: {}", images::image_store_dir().display());
            println!("Cache: {}", config::cache_dir().display());
            println!("Logs: {}", config::logs_dir().display());
            println!("Runtime state: {}", config::runtime_dir().display());
        }
        Commands::Paths => {
            println!("NELRoot paths");
            println!("-------------");
            println!("Data directory : {}", config::data_dir().display());
            println!("Configuration  : {}", config::config_path().display());
            println!("Image catalog  : {}", images::catalog_path().display());
            println!("Image storage  : {}", images::image_store_dir().display());
            println!("Cache          : {}", config::cache_dir().display());
            println!("Logs           : {}", config::logs_dir().display());
            println!("Runtime state  : {}", config::runtime_dir().display());
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
            ImageCommands::Search { query, registry: registry_url } => {
                let matches = registry::search(&registry_url, &query)?;
                if matches.is_empty() {
                    println!("No images matched '{query}'.");
                } else {
                    for image in matches {
                        println!("{}\t{}\t{}", image.name, image.architecture, image.description);
                    }
                }
            }
            ImageCommands::Pull { name, registry: registry_url, architecture } => {
                let rootfs = registry::pull(&registry_url, &name, &architecture)?;
                let _ = logging::record("info", &format!("image pulled: {name} ({architecture})"));
                println!("Image '{name}' downloaded, SHA-256 verified and imported.");
                println!("Root filesystem: {}", rootfs.display());
                println!("Architecture: {architecture}");
            }
            ImageCommands::Download { url, output, sha256 } => {
                let digest = images::download_archive(&url, &output, &sha256)?;
                let _ = logging::record("info", &format!("image archive downloaded: {}", output.display()));
                println!("Image archive downloaded and verified.");
                println!("File: {}", output.display());
                println!("SHA-256: {digest}");
            }
            ImageCommands::Import { name, archive, architecture } => {
                let rootfs = images::import_archive(&name, &archive, &architecture)?;
                let _ = logging::record("info", &format!("image imported: {name} ({architecture})"));
                println!("Imported image '{name}'.");
                println!("Root filesystem: {}", rootfs.display());
                println!("Architecture: {architecture}");
            }
            ImageCommands::Validate { path } => {
                let rootfs = images::validate_rootfs(&path)?;
                println!("Root filesystem validation: passed");
                println!("Path: {}", rootfs.display());
            }
            ImageCommands::Register { name, path, architecture } => {
                images::register_image(&name, &path, &architecture)?;
                let _ = logging::record("info", &format!("image registered: {name} ({architecture})"));
                println!("Registered image '{name}'.");
                println!("Root filesystem: {}", path.canonicalize()?.display());
                println!("The source directory was not copied or modified.");
            }
            ImageCommands::Remove { name } => {
                if images::unregister_image(&name)? {
                    let _ = logging::record("info", &format!("image unregistered: {name}"));
                    println!("Unregistered image '{name}'.");
                    println!("Its files were left untouched.");
                } else {
                    return Err(format!("image '{name}' is not registered").into());
                }
            }
            ImageCommands::List => {
                let catalog = images::load_catalog()?;
                if catalog.images.is_empty() {
                    println!("No images registered.");
                    println!("Search a registry with: nelroot image search ubuntu --registry <https-index-url>");
                } else {
                    for image in catalog.images {
                        println!("{}\t{}\t{}", image.name, image.architecture, image.format);
                    }
                }
            }
            ImageCommands::Inspect { name } => {
                let catalog = images::load_catalog()?;
                match catalog.images.iter().find(|image| image.name == name) {
                    Some(image) => {
                        println!("Image: {}", image.name);
                        println!("Architecture: {}", image.architecture);
                        println!("Format: {}", image.format);
                        let source = image.source.as_deref().unwrap_or("not specified");
                        println!("Root filesystem: {source}");
                        println!("Path exists: {}", std::path::Path::new(source).is_dir());
                    }
                    None => return Err(format!("image '{name}' is not registered").into()),
                }
            }
            ImageCommands::Clean => {
                let removed = registry::clean_staging()?;
                let _ = logging::record("info", &format!("image cleanup removed {removed} staging directories"));
                println!("Removed {removed} abandoned staging directories.");
                println!("Registered images and root filesystems were not modified.");
            }
        },
        Commands::Engine { command } => match command {
            EngineCommands::Status => {
                println!("NELRoot engine status");
                println!("---------------------");
                println!("Core             : available");
                println!("Host detection   : available");
                println!("Configuration    : available");
                println!("Image catalog    : available");
                println!("Registry client  : available");
                println!("Event logging    : available");
                println!("Native executor  : not implemented");
                println!("ELF loader       : not implemented");
                println!("Syscall layer    : not implemented");
                println!("CPU emulator     : not implemented");
                println!("Linux guest boot : not available");
            }
        },
    }

    Ok(())
}
