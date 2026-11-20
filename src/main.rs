mod cli;
mod core;

use clap::Parser;

fn main() {
    let cli = cli::Cli::parse();

    if let Err(error) = cli::run() {
        eprintln!("nelroot: error: {error}");
        std::process::exit(1);
    }
}
