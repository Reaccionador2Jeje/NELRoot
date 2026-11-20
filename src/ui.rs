use std::io::IsTerminal;

fn enabled() -> bool {
    std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none()
}

fn paint(value: &str, code: &str) -> String {
    if enabled() {
        format!("\x1b[{code}m{value}\x1b[0m")
    } else {
        value.to_owned()
    }
}

pub fn banner(subtitle: &str) {
    println!("{}", paint("╭──────────────────────────────────────────────╮", "36"));
    println!("{}", paint("│                 NELRoot                      │", "1;36"));
    println!("{}", paint("│       Independent Linux Runtime              │", "36"));
    println!("{}", paint("╰──────────────────────────────────────────────╯", "36"));
    if !subtitle.is_empty() {
        println!("  {}", paint(subtitle, "2"));
    }
}

pub fn heading(value: &str) {
    println!("\n{}", paint(value, "1;36"));
    println!("{}", paint("─".repeat(value.chars().count().min(60)).as_str(), "2;36"));
}

pub fn key(label: &str, value: impl std::fmt::Display) {
    println!("  {:22} {}", paint(label, "2"), value);
}

pub fn good(value: &str) {
    println!("{} {}", paint("✓", "1;32"), value);
}

pub fn warn(value: &str) {
    println!("{} {}", paint("!", "1;33"), value);
}

pub fn bad(value: &str) {
    eprintln!("{} {}", paint("✗", "1;31"), value);
}

pub fn muted(value: &str) {
    println!("{}", paint(value, "2"));
}

pub fn state(label: &str, available: bool, detail: &str) {
    let marker = if available { paint("●", "32") } else { paint("○", "33") };
    let status = if available { paint("available", "32") } else { paint("not implemented", "33") };
    println!("  {marker} {:24} {:20} {}", label, status, paint(detail, "2"));
}
