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
static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);

#[cfg(target_os = "linux")]
extern "C" fn handle_shutdown_signal(_: libc::c_int) {
    SHUTDOWN_REQUESTED.store(true, Ordering::SeqCst);
}

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
    fs::set_permissions(config::runtime_dir(), fs::Permissions::from_mode(0o700))?;
    SHUTDOWN_REQUESTED.store(false, Ordering::SeqCst);
    unsafe {
        libc::signal(libc::SIGINT, handle_shutdown_signal as libc::sighandler_t);
        libc::signal(libc::SIGTERM, handle_shutdown_signal as libc::sighandler_t);
    }
    let lock = lock_path();
    let lock_file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock)?;
    fs::set_permissions(&lock, fs::Permissions::from_mode(0o600))?;
    // flock is released by the kernel when the daemon exits or crashes, so a
    // stale lock file never prevents the next daemon from starting.
    let lock_result = unsafe { libc::flock(std::os::fd::AsRawFd::as_raw_fd(&lock_file), libc::LOCK_EX | libc::LOCK_NB) };
    if lock_result != 0 {
        return Err("another NELRoot service daemon already owns the runtime lock".into());
    }
    let _lock_guard = LockGuard { _file: lock_file };

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
    listener.set_nonblocking(true)?;
    let mut runtime = ServiceRuntime::new();

    while !SHUTDOWN_REQUESTED.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                let _ = handle_connection(stream, &mut runtime);
                let _ = runtime.poll();
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                let _ = runtime.poll();
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => {
                let _ = fs::remove_file(&socket);
                return Err(error.into());
            }
        }
    }

    // Dropping runtime terminates/reaps owned guest process groups before the
    // control socket and lock are released.
    drop(runtime);
    let _ = fs::remove_file(socket);
    Ok(())
}

#[cfg(target_os = "linux")]
struct LockGuard {
    _file: fs::File,
}

#[cfg(target_os = "linux")]
fn handle_connection(
    mut stream: UnixStream,
    runtime: &mut ServiceRuntime,
) -> Result<(), Box<dyn std::error::Error>> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
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
