//! Local-only NELRoot Service Daemon control plane.
//!
//! The daemon owns an in-memory ServiceRuntime and accepts one JSON request per
//! Unix-domain-socket connection. The socket is created with mode 0600; no TCP
//! listener or guest port publishing is involved. Runtime state is not durable:
//! stopping the daemon terminates its owned guest processes.

#[cfg(target_os = "linux")]
use crate::core::{
    config,
    service_manifest::ServiceManifest,
    service_manager::{ServiceLogEntry, ServiceRuntime, ServiceStatus},
};
#[cfg(target_os = "linux")]
use serde::{Deserialize, Serialize};
#[cfg(target_os = "linux")]
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    os::unix::{
        fs::{FileTypeExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::PathBuf,
};

#[cfg(target_os = "linux")]
const MAX_REQUEST_BYTES: u64 = 1024 * 1024;

#[cfg(target_os = "linux")]
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum DaemonRequest {
    Register {
        environment: String,
        manifest: ServiceManifest,
    },
    Start {
        environment: String,
        service: String,
    },
    Stop {
        environment: String,
        service: String,
    },
    Restart {
        environment: String,
        service: String,
    },
    Status {
        environment: String,
        service: String,
    },
    List {
        environment: String,
    },
    Logs {
        environment: String,
        service: String,
    },
}

#[cfg(target_os = "linux")]
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "result", content = "data", rename_all = "snake_case")]
pub enum DaemonResponse {
    Ok,
    Status(ServiceStatus),
    Statuses(Vec<ServiceStatus>),
    Logs(Vec<ServiceLogEntry>),
    Error(String),
}

#[cfg(target_os = "linux")]
pub fn socket_path() -> PathBuf {
    config::runtime_dir().join("service-daemon.sock")
}

#[cfg(target_os = "linux")]
fn lock_path() -> PathBuf {
    config::runtime_dir().join("service-daemon.lock")
}

/// Run in the foreground. The caller should supervise this process with the
/// operating system's service manager; this function never daemonizes itself.
#[cfg(target_os = "linux")]
pub fn run_foreground() -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(config::runtime_dir())?;
    let lock = lock_path();
    let _lock_file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock)
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                "NELRoot service daemon lock already exists; check whether the daemon is running"
                    .to_owned()
            } else {
                format!("could not acquire service daemon lock: {error}")
            }
        })?;
    let _lock_guard = LockGuard { path: lock };

    let socket = socket_path();
    if socket.exists() {
        let metadata = fs::symlink_metadata(&socket)?;
        if !metadata.file_type().is_socket() {
            return Err("refusing to replace a non-socket service daemon path".into());
        }
        // A stale socket may be removed only after the exclusive daemon lock
        // was acquired, so a second daemon cannot unlink a live listener.
        fs::remove_file(&socket)?;
    }

    let listener = UnixListener::bind(&socket)?;
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600))?;
    let mut runtime = ServiceRuntime::new();

    for incoming in listener.incoming() {
        match incoming {
            Ok(stream) => {
                let _ = handle_connection(stream, &mut runtime);
                let _ = runtime.poll();
            }
            Err(error) => {
                let _ = fs::remove_file(&socket);
                return Err(error.into());
            }
        }
    }

    let _ = fs::remove_file(socket);
    Ok(())
}

#[cfg(target_os = "linux")]
struct LockGuard {
    path: PathBuf,
}

#[cfg(target_os = "linux")]
impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(target_os = "linux")]
fn handle_connection(
    mut stream: UnixStream,
    runtime: &mut ServiceRuntime,
) -> Result<(), Box<dyn std::error::Error>> {
    let cloned = stream.try_clone()?;
    let mut reader = BufReader::new(cloned).take(MAX_REQUEST_BYTES + 1);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    if line.len() as u64 > MAX_REQUEST_BYTES {
        write_response(&mut stream, DaemonResponse::Error("request exceeds 1 MiB".to_owned()))?;
        return Ok(());
    }
    let request = match serde_json::from_str::<DaemonRequest>(&line) {
        Ok(request) => request,
        Err(error) => {
            write_response(
                &mut stream,
                DaemonResponse::Error(format!("invalid request: {error}")),
            )?;
            return Ok(());
        }
    };

    let response = match request {
        DaemonRequest::Register { environment, manifest } => runtime
            .register_server_manifest(&environment, &manifest)
            .map(DaemonResponse::Statuses)
            .map_err(|error| error.to_string()),
        DaemonRequest::Start { environment, service } => runtime
            .start(&environment, &service)
            .map(DaemonResponse::Status)
            .map_err(|error| error.to_string()),
        DaemonRequest::Stop { environment, service } => runtime
            .stop(&environment, &service)
            .map(DaemonResponse::Status)
            .map_err(|error| error.to_string()),
        DaemonRequest::Restart { environment, service } => runtime
            .restart(&environment, &service)
            .map(DaemonResponse::Status)
            .map_err(|error| error.to_string()),
        DaemonRequest::Status { environment, service } => runtime
            .status(&environment, &service)
            .map(DaemonResponse::Status)
            .map_err(|error| error.to_string()),
        DaemonRequest::List { environment } => runtime
            .list(&environment)
            .map(DaemonResponse::Statuses)
            .map_err(|error| error.to_string()),
        DaemonRequest::Logs { environment, service } => runtime
            .logs(&environment, &service)
            .map(DaemonResponse::Logs)
            .map_err(|error| error.to_string()),
    }
    .unwrap_or_else(DaemonResponse::Error);

    write_response(&mut stream, response)?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn write_response(
    stream: &mut UnixStream,
    response: DaemonResponse,
) -> Result<(), Box<dyn std::error::Error>> {
    serde_json::to_writer(&mut *stream, &response)?;
    stream.write_all(b"\n")?;
    stream.flush()?;
    Ok(())
}

/// Send one local daemon request and decode its response.
#[cfg(target_os = "linux")]
pub fn request(request: &DaemonRequest) -> Result<DaemonResponse, Box<dyn std::error::Error>> {
    use std::os::unix::net::UnixStream;

    let mut stream = UnixStream::connect(socket_path()).map_err(|error| {
        format!(
            "NELRoot service daemon is unavailable at {}: {error}",
            socket_path().display()
        )
    })?;
    serde_json::to_writer(&mut stream, request)?;
    stream.write_all(b"\n")?;
    stream.flush()?;

    let mut reader = BufReader::new(stream).take(MAX_REQUEST_BYTES + 1);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    if line.len() as u64 > MAX_REQUEST_BYTES {
        return Err("service daemon response exceeds the protocol limit".into());
    }
    let response: DaemonResponse = serde_json::from_str(&line)?;
    if let DaemonResponse::Error(message) = &response {
        return Err(message.clone().into());
    }
    Ok(response)
}
