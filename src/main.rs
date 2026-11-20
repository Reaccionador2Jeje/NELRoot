mod cli;
mod core;

fn main() {
    if let Err(error) = cli::run() {
        eprintln!("nelroot: error: {error}");
        std::process::exit(1);
    }
}
