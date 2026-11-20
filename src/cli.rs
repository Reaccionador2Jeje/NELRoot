use clap::{Parser, Subcommand};

use crate::core::{host, images, config};

#[derive(Debug, Parser)]
#[command(
    name = "nelroot",
    version,
    about = "Independent Linux execution and virtualization engine",
    long_about = "NELRoot is an independent systems project by AtDev. It is designed to develop its own image, execution and CPU-emulation technologies."
)]
pub struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Display NELRoot and host information.
    Info,
    /// Inspect host capabilities and report development limitations.
    Doctor,
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
    /// Create the default user configuration file.
    Init,
}

#[derive(Debug, Subcommand)]
enum ImageCommands {
    /// List images registered in the local image catalog.
    List,
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
            println!("Execution engine : not implemented yet");
            println!("CPU emulator     : not implemented yet");
            println!("Note             : host detection does not imply guest compatibility");
        }
        Commands::Image { command } => match command {
            ImageCommands::List => {
                let catalog = images::load_catalog()?;
                if catalog.images.is_empty() {
                    println!("No images registered.");
                } else {
                    for image in catalog.images {
                        println!("{}\t{}\t{}", image.name, image.architecture, image.format);
                    }
                }
            }
        },
        Commands::Engine { command } => match command {
            EngineCommands::Status => {
                println!("NELRoot engine status");
                println!("---------------------");
                println!("Core             : initialized");
                println!("Image catalog    : available");
                println!("Native executor  : planned");
                println!("ELF loader       : planned");
                println!("Syscall layer    : planned");
                println!("CPU emulator     : planned");
            }
        },
        Commands::Init => {
            let path = config::initialize()?;
            println!("Created configuration: {}", path.display());
        }
    }

    Ok(())
}
