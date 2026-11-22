# NELRoot Server Environments

**Status: initial metadata manager implemented; guest runtime and server services are not implemented.**

This document defines the direction for turning a registered NELRoot distribution into a managed Linux environment that can host long-running services. The current CLI can create, list, inspect and remove environment definitions backed by registered images. It does not boot guests or provide an SSH daemon, service manager, port forwarding, or a production server sandbox.

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
3. **Service supervisor** tracks guest service processes, readiness, exit codes, logs and restart policy.
4. **Network manager** owns opt-in network isolation and explicit port mappings.
5. **Storage manager** provides explicit persistent data mounts and prevents accidental host-path exposure.
6. **CLI/API** exposes typed operations and machine-readable status.

The service supervisor must not pretend that a daemon is running merely because a command was spawned. It must retain process identity, observe exit, reap children, and recover stale state after an interrupted host session.

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

## Required runtime gates

The current rootless launcher is not yet a complete server runtime. Server mode must remain unavailable until these prerequisites are implemented and tested:

- [ ] PID namespace supervision with guest PID 1 lifecycle, child reaping and signal forwarding.
- [ ] Per-environment process ownership and reliable start/stop/restart.
- [ ] Resource limits for memory, CPU, process count, file descriptors and storage.
- [ ] Guest `/proc`, minimal `/dev` and required runtime mounts.
- [ ] Opt-in network namespace isolation; no silent host-network sharing.
- [ ] Explicit, validated port forwarding with bind-address restrictions.
- [ ] Persistent storage mounts with path validation and ownership rules.
- [ ] Service health checks, bounded logs and stale-session recovery.
- [ ] Integration tests for Debian, Ubuntu and Alpine root filesystems.
- [ ] Security review before allowing untrusted images or remote users.

A namespace capability probe is not proof that a guest server can be safely started. NELRoot must fail closed if any required isolation or supervision step is unavailable. It must not silently fall back to an ordinary chroot or PRoot while reporting server isolation.

## Android and Termux

Termux/PRoot can be useful for CLI development, image inspection and compatibility experiments. It does not provide the kernel namespace and mount guarantees required by the current rootless launcher, and Android may suspend or terminate background processes. Therefore:

- Do not advertise a PRoot guest as a securely isolated NELRoot server.
- Do not assume SSH or other daemons remain alive after Android suspends Termux.
- Do not expose a PRoot guest service to the public Internet.
- Report host capability limitations explicitly and keep unsupported server actions disabled.

## Milestones

1. **Contract and initial environment registry:** implemented metadata definitions and CRUD CLI; a separate runtime capability report remains pending.
2. **Supervised environment lifecycle:** implement start/stop/status and reliable process ownership.
3. **Service management:** implement service definitions, readiness, logs and bounded restart behavior.
4. **Network isolation:** add opt-in network namespaces and explicit local-only port forwarding.
5. **Guest SSH:** add a tested SSH service workflow with per-guest keys and loopback-first exposure.
6. **Persistent server environments:** add explicit data mounts, recovery and distribution integration tests.
7. **Remote control:** expose a narrowly scoped authenticated API only after the isolation gates pass.

Until these milestones are complete, NELRoot remains an early-development execution engine and must not be described as a production container or public server platform.
