use std::{
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use super::config;

/// Append a timestamped, single-line event to the local NELRoot log.
pub fn record(level: &str, message: &str) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(config::logs_dir())?;
    let path = log_path();
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let safe_message = message.replace(['\\n', '\\r'], " ");
    writeln!(file, "{timestamp} [{level}] {safe_message}")?;
    Ok(())
}

pub fn log_path() -> PathBuf {
    config::logs_dir().join("nelroot.log")
}

/// Read only the requested tail of the event log.
pub fn tail(lines: usize) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let path = log_path();
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file = fs::File::open(path)?;
    let all = BufReader::new(file)
        .lines()
        .collect::<Result<Vec<_>, _>>()?;
    let start = all.len().saturating_sub(lines);
    Ok(all.into_iter().skip(start).collect())
}
