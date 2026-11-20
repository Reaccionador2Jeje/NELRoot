# NEXORA ↔ NELRoot Integration Contract

This document defines the integration boundary for replacing NEXORA's PRoot-based Ubuntu provider with the independent NELRoot runtime.

## Non-negotiable architecture

- NELRoot owns the guest runtime, executable loading, syscall compatibility and process lifecycle.
- NEXORA must not invoke PRoot, QEMU or Docker as a hidden NELRoot backend.
- NEXORA consumes a stable NELRoot CLI/API contract; it must not manipulate NELRoot's internal catalog files directly.
- An imported or registered root filesystem is not considered runnable until the runtime reports that capability.
- NEXORA must preserve a migration and rollback path until parity tests pass.

## Planned command contract

The following environment lifecycle commands remain target interface, not currently implemented:

```text
nelroot env create <name> --image <image>
nelroot env start <name>
nelroot env stop <name>
nelroot env shell <name>
nelroot env exec <name> -- <command> [args...]
nelroot env inspect <name> --json
nelroot env list --json
nelroot env remove <name>
nelroot port expose <name> <guest-port> [--host-port <port>]
nelroot package <project-path> --output <artifact>
```

The exact syntax may evolve before the full runtime milestone. A first Linux-only process launcher is now available through `nelroot run <image> -- <absolute-program> [args...]` and `nelroot shell <image>`. It requires effective UID 0 plus effective `CAP_SYS_CHROOT`, `CAP_SETUID` and `CAP_SETGID` capabilities, supports same-architecture images only, and uses the host kernel for ELF loading and syscalls. It is not yet the independent guest ABI runtime or a secure sandbox. NEXORA can use `nelroot --version`, `nelroot doctor` and `nelroot engine status --json` for capability detection.

## Required machine-readable response

The first versioned machine-readable probe is implemented:

```bash
nelroot engine status --json
```

It returns `schema_version`, product/version, host OS and architecture, a component status array, and `guest_execution.available`, along with the native backend name and same-architecture requirement. NEXORA should check `schema_version == 1` and treat `guest_execution.available == false` as a hard stop for guest operations. A true value means the Linux chroot backend has a plausible privilege context; it does not mean the full independent runtime or a security sandbox is available.

Before environment lifecycle integration, add versioned JSON responses for environment listing, inspection, process execution and errors. Each environment operation should report:

- API/schema version.
- Environment identifier and lifecycle state.
- Image identity and architecture.
- Working directory and persistent mount information.
- Process ID (when running), exit code and termination reason.
- Port mappings and networking state.
- Structured error code, human-readable message and recovery hint.

Do not parse human-oriented terminal output as an API.

## Runtime readiness gates

NEXORA must not switch its default provider until all of these are demonstrated:

1. Independent NELRoot runtime can start and stop a supported Linux guest.
2. Interactive shell and non-interactive command execution work.
3. File persistence and working-directory semantics are stable.
4. Signals, process exit codes and cleanup are correct.
5. A local web server can bind inside the environment and be reached through an explicitly managed port.
6. Package installation and a representative project build pass.
7. Host access, privilege boundaries and isolation limits are documented.
8. Android ARM64 and other claimed platforms pass their own compatibility tests.

## Migration sequence

1. Add a NEXORA adapter behind a feature flag; keep the existing provider untouched.
2. Run NELRoot compatibility checks against disposable test projects.
3. Compare command output, filesystem persistence, exit codes and server reachability.
4. Allow opt-in NELRoot environments for development/testing.
5. Fix parity gaps and run soak tests.
6. Switch defaults only after the readiness gates pass.
7. Retain rollback to the previous provider during the transition.

## Current status

NELRoot provides CLI, host diagnostics, configuration, local image catalog, HTTPS registry search/pull and image import, plus an initial Linux chroot launcher for same-architecture commands. The host kernel still handles ELF loading and syscalls; process lifecycle management, VFS, namespace isolation, independent ELF loading and syscall compatibility are not implemented. Production replacement of NEXORA’s PRoot backend is not ready.
