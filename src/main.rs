mod cli;
mod core;

fn main() {
    if let Err(error) = cli::run() {
        let _ = core::logging::record("error", &error.to_string());
        eprintln!("nelroot: error: {error}");
        std::process::exit(1);
    }
}
