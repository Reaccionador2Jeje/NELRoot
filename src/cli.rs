use clap::{Parser, Subcommand};

use crate::core::{config, host, images};

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
    /// List images registered in the local image catalog.
    List,
    /// Inspect an image registered in the local catalog.
    Inspect {
        /// Image name from the local catalog.
        name: String,
    },
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
        }
        Commands::Doctor => {
            println!("NELRoot host diagnostics");
            println!("------------------------");
            println!("Operating system : {}", host::os_name());
            println!("Architecture     : {}", host::architecture());
            println!("Pointer width    : {}-bit", usize::BITS);
            println!("Config directory : {}", config::data_dir().display());
            println!("Execution engine : under development");
            println!("CPU emulator     : under development");
            println!("Note             : host detection does not imply guest compatibility");
        }
        Commands::Setup | Commands::Init => {
            let path = config::initialize()?;
            println!("NELRoot environment initialized.");
            println!("Configuration: {}", path.display());
            println!("Image catalog: {}", images::catalog_path().display());
        }
        Commands::Paths => {
            println!("NELRoot paths");
            println!("-------------");
            println!("Data directory : {}", config::data_dir().display());
            println!("Configuration : {}", config::data_dir().join("config.toml").display());
            println!("Image catalog : {}", images::catalog_path().display());
        }
        Commands::Config { command } => match command {
            ConfigCommands::Path => {
                println!("{}", config::data_dir().join("config.toml").display());
            }
            ConfigCommands::Show => {
                let path = config::data_dir().join("config.toml");
                if !path.exists() {
                    println!("Configuration not initialized. Run nelroot setup first.");
                } else {
                    print!("{}", std::fs::read_to_string(path)?);
                }
            }
        },
        Commands::Image { command } => {
            let catalog = images::load_catalog()?;
            match command {
                ImageCommands::List => {
                    if catalog.images.is_empty() {
                        println!("No images registered.");
                        println!("Image importing is planned for a future release.");
                    } else {
                        for image in catalog.images {
                            println!("{}\t{}\t{}", image.name, image.architecture, image.format);
                        }
                    }
                }
                ImageCommands::Inspect { name } => {
                    match catalog.images.iter().find(|image| image.name == name) {
                        Some(image) => {
                            println!("Image: {}", image.name);
                            println!("Architecture: {}", image.architecture);
                            println!("Format: {}", image.format);
                            println!("Source: {}", image.source.as_deref().unwrap_or("not specified"));
                        }
                        None => return Err(format!("image '{name}' is not registered").into()),
                    }
                }
            }
        }
        Commands::Engine { command } => match command {
            EngineCommands::Status => {
                println!("NELRoot engine status");
                println!("---------------------");
                println!("Core             : initialized");
                println!("Host detection   : available");
                println!("Image catalog    : available");
                println!("Native executor  : planned");
                println!("ELF loader       : planned");
                println!("Syscall layer    : planned");
                println!("CPU emulator     : planned");
            }
        },
    }

    Ok(())
}
