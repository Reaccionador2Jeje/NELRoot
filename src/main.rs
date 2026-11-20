mod cli;
mod ui;

fn main() {
    if let Err(error) = cli::run() {
        let _ = nelroot::core::logging::record("error", &error.to_string());
        crate::ui::bad(&format!("nelroot: {error}"));
        std::process::exit(1);
    }
}
