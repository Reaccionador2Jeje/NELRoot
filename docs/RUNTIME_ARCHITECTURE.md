# NELRoot Runtime Architecture (Design)

This document describes the implementation path for a genuinely independent Linux userspace runtime. It is a design contract, not a claim that these components already exist.

## Constraints

- No PRoot, QEMU or Docker as an execution backend.
- No silent substitution of host commands for guest commands.
- Host kernel features are capabilities, not assumptions.
- Rootless operation is a goal where the host OS permits it, not a universal guarantee.
- A rootfs being imported or registered does not make it executable.
- Runtime capability is reported explicitly per host and architecture.

## Implemented initial execution backend

The repository now contains a Linux-only native process launcher in `src/core/runtime.rs`. It resolves a registered image, rejects cross-architecture launches, starts a child, calls `chroot(2)`, changes to a validated guest working directory, drops supplementary groups and credentials to UID/GID 65534 by default, enables `no_new_privs`, and executes an absolute guest path. It propagates the child exit status.

This is a genuine native process launch from the selected rootfs and does not invoke PRoot, QEMU or Docker. The host Linux kernel performs ELF loading and syscall handling. The initial implementation requires effective root and `CAP_SYS_CHROOT`, and is not available in ordinary unprivileged PRoot sessions or typical Termux installations.

**Security limitation:** chroot alone is not a sandbox. This backend does not create mount, PID, user or network namespaces, mount virtual filesystems, filter networking, or isolate kernel attack surface. The explicit `--keep-root` option retains host root credentials and is unsafe for untrusted images. See [NATIVE_RUNTIME.md](NATIVE_RUNTIME.md).

## Layered design

```text
NELRoot CLI / NEXORA adapter
          |
Runtime Manager (environment lifecycle, sessions, process table)
          |
Guest ABI (ELF loader, dynamic linker, syscall dispatcher)
          |
Virtual Kernel Services (VFS, fd table, signals, memory, processes)
          |
Host Capability Adapter (Linux / Android / Windows / macOS)
          |
Host OS kernel
```

### 1. Runtime manager

Owns environment metadata, start/stop transitions, session IDs, process supervision, logs and recovery after interrupted runs. Each environment has a stable ID, rootfs reference, architecture, working directory and lifecycle state.

State transitions must be validated:

```text
Created -> Starting -> Running -> Stopping -> Stopped
                 |                    |
                 +----> Failed <------+
```

### 2. Virtual filesystem

The VFS maps guest absolute paths into an environment root and manages mount points. Path resolution must reject traversal outside the guest root unless an explicit mount grants access. File descriptors are represented by NELRoot-owned handles rather than leaking arbitrary host descriptors.

Initial filesystem milestones:

1. Read-only rootfs inspection.
2. Guest path normalization and root confinement.
3. Read-only file open/stat/readdir.
4. Writable overlay and persistent project mounts.
5. Symlink, permissions, ownership and special-file compatibility.

### 3. ELF loader

The loader must parse ELF headers and program headers with bounds checks, validate machine/ABI fields, map PT_LOAD segments with correct permissions, establish stack and auxiliary vectors, and locate the requested interpreter. Malformed binaries must fail closed.

Milestones:

1. ELF64 little-endian parser for a single supported architecture.
2. Static PIE/non-PIE loading where feasible.
3. Initial process stack, argv, envp and auxv.
4. Dynamic linker handoff and shared-object resolution.
5. Relocations, TLS and architecture-specific ABI details.

### 4. Syscall compatibility

Syscalls are dispatched through a versioned guest ABI table. Each syscall implementation must define argument validation, guest memory access, return/error translation, interruption behavior and tests. Unsupported syscalls return a documented guest error; they must never silently call an unrelated host operation.

Start with a deliberately small, testable set: process exit, read/write, close, openat, fstat, mmap/munmap, brk, clock_gettime, getpid, getuid/getgid, and basic directory operations. Expand only with conformance tests.

### 5. Process, signals and memory

A guest process model owns registers, address space, descriptors, cwd, credentials and signal state. Thread creation, fork/clone semantics, wait, signal delivery and robust memory isolation are separate milestones. Do not claim multi-process Linux compatibility until lifecycle and signal tests pass.

### 6. Architecture execution

Same-architecture execution and cross-architecture emulation are separate features. The initial milestone should target one architecture and a narrow ABI. Cross-architecture support requires an independently implemented instruction decoder, execution engine, memory model and exception behavior; it must not be represented by an external QEMU invocation.

### 7. Networking and ports

Networking requires an explicit guest socket model, host transport adapter, address/port policy and lifecycle cleanup. Port exposure must be opt-in, bound to a selected host interface, and reported to NEXORA. A running web server must be tested end-to-end before the capability is advertised.

## Required internal interfaces

Keep implementation modules narrow and testable:

- `RuntimeManager`: create/start/stop/inspect/list environments.
- `GuestProcess`: execute, wait, signal and report exit state.
- `ElfLoader`: validate and prepare an ELF image.
- `SyscallDispatcher`: map guest syscall numbers to implementations.
- `VirtualFileSystem`: resolve guest paths and file handles.
- `HostCapabilities`: report available kernel interfaces and restrictions.

These interfaces should return typed errors with stable codes. CLI text is for humans; NEXORA must eventually consume a versioned JSON interface.

## Test gates

- ELF parser fuzzing and malformed-input tests.
- VFS traversal, symlink and permission tests.
- Syscall conformance tests against small purpose-built guest binaries.
- Process exit, signal and descriptor lifecycle tests.
- Memory mapping and guard-page tests.
- Rootless Android ARM64 capability tests.
- Linux x86_64 and AArch64 compatibility suites.
- Network port lifecycle and isolation tests.

## First implementation milestone

The ELF64 inspection parser is implemented in `src/core/elf.rs`; the non-executing planner is in `src/core/loader.rs`. A first Linux memory mapper is now implemented in `src/core/memory.rs` and exposed as `nelroot elf map <file>`. It maps bounded, host-native static ET_DYN images into the current process, populates PT_LOAD bytes, relies on anonymous zero pages for BSS, applies per-page permissions, rejects W+X pages and overlapping segment byte ranges, validates PT_LOAD alignment, and unmaps through RAII. Its `read_bytes` and `write_bytes` APIs check PF_R/PF_W permissions across the complete requested range before accessing mapped memory. It deliberately does not execute the entry point, apply relocations, load PT_INTERP, or establish a guest stack. Dynamic linking, relocation processing, auxv/argv/envp stack construction, and the syscall ABI remain future milestones.
