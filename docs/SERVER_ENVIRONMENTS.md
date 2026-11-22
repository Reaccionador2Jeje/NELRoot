# NELRoot Server Environments

**Status: environment metadata, validated service manifests, a Linux rootless service-process adapter and a local Unix-socket service daemon with CLI controls are implemented. Durable state, readiness, output capture and automatic restart scheduling remain pending.**

This document defines the direction for turning a registered NELRoot distribution into a managed Linux environment that can host long-running services. The current CLI can create, list, inspect and remove environment definitions backed by registered images. The Linux CLI can now start a foreground daemon that owns the service runtime and accepts local CLI control requests. SSH setup, port forwarding and production server operation remain unavailable.

## Goals

A named environment should eventually support:

- An interactive guest shell and explicit command execution.
- Long-running guest processes with observable lifecycle and exit status.
- Guest services such as SSH, HTTP servers, application workers and databases.
- Service inspection, start, stop, restart and logs.
- Explicit network exposure and port-forwarding policies.
- Persistent guest data with clearly defined ownership and mount permissions.
- Host-independent operation when the machine running NELRoot remains available.

The guest SSH service is distinct from the host's SSH daemon. A guest SSH server must authenticate users belonging to that guest and must not grant access to the host or sibling environments.

## Implemented CLI: environment definitions

These commands are implemented. They manage metadata only; `create` does not start a guest process or install services:

```text
nelroot server create <image> --name <environment>
nelroot server list
nelroot server inspect <environment>
nelroot server remove <environment>
```

Removing an environment definition never deletes its registered image or root filesystem.

## Service manifest contract

The first versioned service-manifest model is implemented in `src/core/service_manifest.rs`. It parses TOML and validates schema version 1, unique service identifiers, guest-absolute executable and working-directory paths, bounded argument/environment counts, and environment-variable names and values. Unknown TOML fields are rejected.

This is a declarative contract only. It does not launch services, read credentials, open ports, or provide a running-state claim. Do not put passwords, private keys, API tokens or other secrets in a manifest; a future secret-injection interface must keep them outside version-controlled configuration.

Example manifest:

```toml
schema_version = 1

[[services]]
name = "web"
command = "/usr/bin/python3"
args = ["-m", "http.server", "8080"]
working_directory = "/srv/site"
restart = "on_failure"
```

## Service manager and isolated process adapter

The service manager in `src/core/service_manager.rs` now has two layers:

- `ServiceManager` is the deterministic in-memory lifecycle/state model.
- On Linux, `ServiceRuntime` connects that model to `runtime::spawn_rootless_image_with_env` and owns the returned `RootlessProcess` handles.

A service start resolves the registered image, validates the manifest and starts the configured absolute guest executable through NELRoot's rootless launcher. The launcher creates private user, mount, PID and network namespaces, uses a read-only image rootfs, applies its configured resource limits and starts the service under a dedicated host process group. Manifest environment variables are passed to the guest after validation.

The runtime adapter supports start, poll/status, stop/reap, and explicit restart of a failed service when the manifest policy permits it. Dropping the in-process `ServiceRuntime` terminates and reaps processes it owns. Stop currently uses immediate process-group termination; graceful signal forwarding is not implemented. Managed services currently inherit the standard 512 MiB address-space, 256-file and 64 MiB per-file limits, with a one-day cumulative CPU-time ceiling; configurable per-service resource profiles are still pending.

## Local service daemon and CLI

On Linux, `src/core/service_daemon.rs` adds a foreground daemon with one JSON request per Unix-domain-socket connection. The socket lives under NELRoot's runtime directory and is restricted to mode `0600`; no TCP control endpoint is opened. An exclusive kernel `flock` prevents multiple daemons from owning the same runtime, and the lock is released automatically if the daemon exits unexpectedly.

Start the daemon in one terminal:

```text
nelroot service daemon
```

Then use another terminal:

```text
nelroot service register <environment> <manifest.toml>
nelroot service start <environment> <service>
nelroot service status <environment> <service>
nelroot service list <environment>
nelroot service logs <environment> <service>
nelroot service stop <environment> <service>
nelroot service restart <environment> <service>
```

The daemon owns the in-memory `ServiceRuntime`; the CLI is a local IPC client. It runs in the foreground and should be supervised by the host's service manager if unattended operation is needed.

**Important boundaries:**

- The daemon keeps services alive while it remains running. If it exits or is terminated, its owned guest processes are stopped and reaped; no durable service recovery exists yet.
- Registration and service state are in memory and must be registered again after daemon restart.
- Service stdout/stderr are still inherited from the daemon; the bounded log buffer is not wired to real process output, so `logs` is currently an empty/manual buffer.
- Readiness probes and automatic restart scheduling are not implemented. `restart` is an explicit request and only works for a failed service whose manifest policy is `on_failure`.
- The guest receives a private network namespace with no automatic host port publishing. The daemon does not expose guest services to the public network.
- The runtime adapter only supports Linux hosts where all required namespace and mount operations are permitted. It fails closed when setup fails.


## Planned CLI: runtime and services

The following syntax remains a design proposal and is not part of the current CLI:

```text
nelroot server create <image> --name <environment>
nelroot server start <environment>
nelroot server status <environment>
nelroot server stop <environment>
nelroot server remove <environment>

nelroot service list <environment>
nelroot service status <environment> <service>
nelroot service start <environment> <service>
nelroot service stop <environment> <service>
nelroot service logs <environment> <service>

nelroot port list <environment>
nelroot port publish <environment> <guest-port> --bind <host-address>
nelroot port unpublish <environment> <guest-port>
```

The final syntax may change as the runtime and lifecycle contracts are implemented.

## Architecture

The server manager is planned as a control plane over the NELRoot runtime, not as a second execution engine:

1. **Environment registry** stores a named environment definition, image reference, rootfs path, architecture, creation time and metadata state. The initial registry is implemented in `src/core/server.rs`.
2. **Runtime adapter** starts and stops the guest using a fully supervised NELRoot process/session.
3. **Service supervisor** now owns rootless guest process handles and observes exit/stop; readiness, real output capture, persistent daemon ownership and automatic restart scheduling remain pending.
4. **Network manager** owns opt-in network isolation and explicit port mappings.
5. **Storage manager** provides explicit persistent data mounts and prevents accidental host-path exposure.
6. **CLI/API** exposes typed operations and machine-readable status.

The in-process service runtime retains process identity, observes exit and reaps children. Durable ownership and recovery after an interrupted host session still require a persistent daemon.

## SSH service model

SSH is an optional guest service, not an automatic host-wide listener.

- SSH must run inside the selected guest environment.
- Guest accounts and authorized keys are managed independently of host accounts.
- The initial safe development mode should bind to loopback only.
- Non-loopback exposure requires an explicit port-publishing operation and a clear authentication configuration.
- Passwords and private keys must never be printed in CLI output or stored in plaintext configuration.
- Host SSH configuration must never be modified by guest service setup.
- A guest must not be able to reach host-only control sockets or other guests by default.

An SSH daemon package being present in a rootfs does not mean NELRoot has implemented or secured guest SSH lifecycle management.

## Current implementation boundary

The first milestone stores server-environment definitions in `~/.nelroot/servers.toml`. It validates the name, requires an already registered image, validates the referenced rootfs, and writes the catalog atomically. `server remove` deletes only the definition. The `created` state means registered metadata, not a running guest.

## Remaining production runtime gates

The rootless launcher now creates private user, mount, PID and network namespaces; runs the guest command as PID 1 under a host-side supervisor; mounts a read-only rootfs, writable bounded `/tmp`, guest `/proc` and a private tmpfs at `/dev`; applies address-space, CPU-time, file-descriptor and file-size limits; and enables `no_new_privs`. The process handle owns a dedicated host process group and reaps the supervisor on exit.

The current service manager can start, inspect, stop and explicitly restart eligible guest processes. The local daemon keeps that manager alive while the daemon runs. These are real Linux runtime operations, but they do not yet make NELRoot a production multi-tenant server platform.

Remaining gates:

- [ ] Graceful signal forwarding and complete PID-1 child supervision semantics.
- [ ] Standard guest device nodes and a broader, distribution-tested `/dev` implementation.
- [ ] Per-service configurable resource profiles, process-count limits and storage quotas.
- [ ] Persistent host data mounts with strict path, ownership and lifecycle rules.
- [x] Private network namespace isolation (guest networking is not published to the host).
- [ ] Explicit, validated port forwarding with loopback-first bind-address restrictions.
- [ ] Real stdout/stderr capture, durable logs, readiness probes and bounded automatic restart scheduling.
- [ ] Durable daemon state and recovery after daemon or host interruption.
- [ ] Integration tests across Debian, Ubuntu and Alpine root filesystems.
- [ ] Security review before allowing untrusted images or remote workloads.

The daemon's Unix socket is local to the host user and is not a remote control API. A namespace capability probe is not proof that every guest workload can start. NELRoot must fail closed if required namespace or mount setup is unavailable; it must never silently fall back to ordinary chroot or PRoot while reporting isolation.

## Android and Termux

Termux/PRoot can be useful for CLI development, image inspection and compatibility experiments. It does not provide the kernel namespace and mount guarantees required by the current rootless launcher, and Android may suspend or terminate background processes. Therefore:

- Do not advertise a PRoot guest as a securely isolated NELRoot server.
- Do not assume SSH or other daemons remain alive after Android suspends Termux.
- Do not expose a PRoot guest service to the public Internet.
- Report host capability limitations explicitly and keep unsupported server actions disabled.

## Milestones

1. **Contract and initial environment registry:** implemented metadata definitions and CRUD CLI; a separate runtime capability report remains pending.
2. **Supervised environment lifecycle:** implement start/stop/status and reliable process ownership.
3. **Service management:** validated manifests, a Linux rootless process adapter, a local persistent-while-running daemon and CLI lifecycle controls are implemented; add readiness, real logs, durable recovery and bounded automatic restart scheduling.
4. **Network isolation:** add opt-in network namespaces and explicit local-only port forwarding.
5. **Guest SSH:** add a tested SSH service workflow with per-guest keys and loopback-first exposure.
6. **Persistent server environments:** add explicit data mounts, recovery and distribution integration tests.
7. **Remote control:** expose a narrowly scoped authenticated API only after the isolation gates pass.

Until these milestones are complete, NELRoot remains an early-development execution engine and must not be described as a production container or public server platform.
