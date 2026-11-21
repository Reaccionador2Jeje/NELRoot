# NELRoot

**Independent Linux Execution & Virtualization Engine**

NELRoot is an open-source systems project by **AtDev**. Its long-term goal is to execute Linux environments and other system images through independently developed runtime, executable-loading, syscall-compatibility, and CPU-emulation components.

NELRoot is designed as an independent engine. PRoot and QEMU are not execution backends or runtime requirements.

> **Development status:** Early development. NELRoot has a Linux-only native chroot launcher, a bounded memory mapper for host-native static PIE images, an initial RELATIVE RELA relocation stage for x86_64/AArch64, and a bounded ELF64 initial-stack image builder. These ELF tools prepare image data but do not execute guest code; the chroot launcher still relies on the host kernel for ELF loading and syscalls. Neither is a complete independent guest runtime or secure sandbox. See [docs/RUNTIME_ARCHITECTURE.md](docs/RUNTIME_ARCHITECTURE.md) for implementation boundaries.

## Product direction

NELRoot is being developed as a modern, friendly Linux environment for developers: interactive shells, Linux commands, project builds, local web servers and project packaging through a simple CLI. The long-term goal is an independent runtime, not a wrapper around PRoot, QEMU or Docker.

A major integration objective is to replace the PRoot-based Ubuntu backend in **NEXORA** with NELRoot after runtime capabilities and compatibility tests are ready. The migration will cover shell sessions, project files, process lifecycle, command execution, web-server ports and build workflows.

NELRoot is also planned for native package-manager distribution: APT (`.deb`), DNF/Zypper (`.rpm`), Pacman (`.pkg.tar.zst`), APK and Termux packages. AtDev-maintained signed repositories are the initial target; inclusion in official distribution repositories requires their separate maintainer review and acceptance.

See [ROADMAP.md](ROADMAP.md) for milestones and the package-distribution plan. The first-party image distribution service is implemented as a separate `nelroot-hub` binary; see [docs/IMAGE_REGISTRIES.md](docs/IMAGE_REGISTRIES.md) for its API, storage layout and publishing workflow. These are future goals, not features available in the current early-development build. The experimental static PIE memory mapper and its limited RELATIVE relocation stage are documented in [docs/ELF_MEMORY_MAPPING.md](docs/ELF_MEMORY_MAPPING.md).

## Install

Download the latest installer from the [GitHub Releases](https://github.com/Reaccionador2Jeje/NELRoot/releases/latest) page.

### Linux, macOS and Android (Termux)

Install the prerequisites and run the setup wizard:

```bash
# Ubuntu / Debian
sudo apt install wget

# Termux
pkg install wget coreutils tar

# Then, on either platform:
wget -O nelroot-install.sh https://raw.githubusercontent.com/Reaccionador2Jeje/NELRoot/main/scripts/install.sh
bash nelroot-install.sh
```

The wizard detects the operating system and CPU architecture, downloads the matching release with `wget`, verifies its SHA-256 checksum using `sha256sum` or `shasum`, asks where to install, and configures the user PATH. If no GitHub Release asset exists (HTTP 404), the wizard downloads the current source snapshot with wget and builds it with Cargo instead. For that fallback, install a current stable Rust toolchain (Rust 1.85 or newer) before running the wizard. Unix-like systems need `wget`, `tar`, and a SHA-256 utility. macOS users can install wget with Homebrew. Choose system-wide installation when prompted to install for all local users; root can install directly, while a non-root user needs sudo. On Termux, the installer writes into the Termux prefix and does not require root.

### Windows (PowerShell)

Open PowerShell and run:

```powershell
[System.IO.File]::WriteAllText("$env:TEMP\\nelroot-install.ps1", (New-Object System.Net.Http.HttpClient).GetStringAsync("https://raw.githubusercontent.com/Reaccionador2Jeje/NELRoot/main/scripts/install.ps1").GetAwaiter().GetResult())
& "$env:TEMP\\nelroot-install.ps1"
```

The wizard uses .NET HttpClient (not curl or wget), verifies the SHA-256 checksum, installs into the current user's Programs directory and adds it to the user's PATH.

### Supported release targets

| Platform | Architecture | Package |
|---|---|---|
| Linux | x86_64 | `nelroot-linux-x86_64.tar.gz` |
| Linux | ARM64 | `nelroot-linux-aarch64.tar.gz` |
| Debian / Ubuntu | x86_64, ARM64 | `.deb` package attached to GitHub Releases |
| Fedora / RHEL / openSUSE | x86_64, ARM64 | `.rpm` package attached to GitHub Releases |
| Android / Termux | ARM64 | `nelroot-android-aarch64.tar.gz` |
| Windows | x86_64 | `nelroot-windows-x86_64.zip` |
| macOS | Intel x86_64 | `nelroot-macos-x86_64.tar.gz` |
| macOS | Apple Silicon ARM64 | `nelroot-macos-aarch64.tar.gz` |

## CLI commands

After installation, open a new terminal:

```bash
nelroot --help
nelroot --version

nelroot info
nelroot doctor
nelroot setup
nelroot paths

nelroot config path
nelroot config show

nelroot image list
nelroot image download <https-url> <archive.tar.gz> --sha256 <64-hex-digest>
nelroot image import <name> <archive.tar.gz> --architecture aarch64
nelroot image validate <rootfs-path>
nelroot image inspect <name>
nelroot image register <name> <rootfs-path> --architecture aarch64
nelroot image remove <name>
nelroot image search ubuntu --registry https://example.org/nelroot/index.json
nelroot image pull ubuntu-noble --registry https://example.org/nelroot/index.json --architecture aarch64
nelroot image update ubuntu-noble --registry https://example.org/nelroot/index.json --architecture aarch64
nelroot image clean

nelroot logs --lines 50
nelroot project create demo
nelroot project list
nelroot elf inspect /path/to/elf-binary
nelroot elf plan /path/to/elf-binary
# Linux only: map a static host-native PIE into memory, without executing it
nelroot elf map /path/to/static-pie
# Linux only: apply supported RELATIVE RELA relocations in memory; no execution
nelroot elf relocate /path/to/static-pie
# Core API: prepare_static_pie() sequences map + RELATIVE RELA + load plan + stack image
nelroot engine status
nelroot engine status --json

# Native Linux execution (Linux host + effective UID 0 and required capabilities)
nelroot run ubuntu -- /bin/uname -a
nelroot shell ubuntu
# Rootless interactive shell (no host root)
nelroot shell-rootless ubuntu
# Share a persistent NELRoot project with the distro at /workspace
nelroot shell-rootless ubuntu --workspace ~/.nelroot/projects/demo --cwd /workspace
# Run a project with tools installed inside the distro
nelroot run-rootless ubuntu --workspace ~/.nelroot/projects/demo --cwd /workspace -- /bin/sh -lc 'make test'
# Run the same project directly on the Linux host (no guest isolation)
nelroot host run --project demo -- make test
# Rootless native launcher (no host root; user, mount and PID namespaces)
nelroot run-rootless ubuntu -- /bin/uname -a
# Optional, unsafe: retain host root credentials inside chroot
nelroot shell ubuntu --keep-root
# Rootless interactive shell (recommended for unprivileged Linux hosts)
nelroot shell-rootless ubuntu
```

Run `nelroot <command> --help` for command-specific help.

### Rootless Linux runtime

Linux hosts can also use NELRoot's independent rootless launcher without host UID 0:

```bash
nelroot run-rootless <image> -- /bin/sh -c 'printf "NELRoot rootless OK\\n"'
nelroot run-rootless <image> -- /bin/uname -m
```

This backend is implemented directly with Linux user, mount and PID namespaces, UID/GID mapping, a persistent writable OverlayFS root, `pivot_root`, guest `/proc`, and `no_new_privs`. The imported image remains the immutable lower layer; writes persist in a per-image upper layer. The guest runs as PID 1 in its PID namespace and its exit status is supervised by NELRoot. It does not invoke PRoot, QEMU or Docker. The invoking host UID/GID remain unprivileged. The current milestone is same-architecture and still lacks guest `/dev` and devpts, isolated networking, resource limits, full signal/descendant lifecycle handling, and complete distribution validation. Kernel, container, seccomp or LSM policy can deny namespace or mount operations; NELRoot fails closed instead of falling back to chroot. See [docs/ROOTLESS_RUNTIME.md](docs/ROOTLESS_RUNTIME.md) for exact limits and prerequisites.

### Native Linux runtime

On Linux hosts with effective UID 0 and the effective `CAP_SYS_CHROOT`, `CAP_SETUID` and `CAP_SETGID` capabilities, NELRoot can launch a native same-architecture program from a registered root filesystem:

```bash
nelroot run ubuntu -- /bin/uname -a
nelroot shell ubuntu
```

The default child drops to UID/GID 65534 and enables `no_new_privs`. The optional `--keep-root` flag is unsafe and must only be used with trusted images. This initial backend uses the host kernel's ELF loader and syscall implementation. It does not provide mount/PID/user/network namespaces or a security sandbox, and it is unavailable in ordinary unprivileged PRoot sessions and typical Termux installations. Read [docs/NATIVE_RUNTIME.md](docs/NATIVE_RUNTIME.md) before using it.

### Remote image registry

NELRoot supports a versioned, HTTPS-only JSON registry index. Registry entries include an image name, description, architecture, HTTPS archive URL and SHA-256 digest. The index is limited to 4 MiB; invalid names, duplicate name/architecture pairs, non-HTTPS URLs and malformed checksums are rejected.

```bash
nelroot image search ubuntu --registry https://example.org/nelroot/index.json
nelroot image pull ubuntu --registry https://example.org/nelroot/index.json --architecture aarch64
```

The registry client verifies the archive SHA-256 before importing it.

For application integrations such as NEXORA, use `nelroot engine status --json`. It returns schema version `1`, host identity, component availability and an explicit `guest_execution.available` boolean. NEXORA can parse this JSON without relying on human-readable terminal output; the response reports whether the native Linux chroot backend can start on this host, and identifies its backend and same-architecture limitation. For first-party image discovery and downloads, use NELRoot Image Hub (`nelroot image hub-remote-search` and `nelroot image hub-pull`). The optional Docker Hub commands only inspect external registry metadata; they do not download Docker layers. See [docs/IMAGE_REGISTRIES.md](docs/IMAGE_REGISTRIES.md) for the first-party API and publishing workflow.

### Local image catalog

NELRoot can register an existing Linux root filesystem directory without copying it:

```bash
nelroot image validate /path/to/rootfs
nelroot image register ubuntu-rootfs /path/to/rootfs --architecture aarch64
nelroot image list
nelroot image inspect ubuntu-rootfs
nelroot image remove ubuntu-rootfs
```

The directory must resemble a Linux root filesystem (for example, it contains `etc`, `usr`, `bin` or `sbin`). Removing an entry only unregisters it; it never deletes the root filesystem.

### ELF64 inspection

NELRoot includes a bounded ELF64 little-endian metadata inspector. It validates header and program-header bounds, load-segment ranges and alignment, and reports the dynamic interpreter (`PT_INTERP`) when present. It never executes the inspected file:

```bash
nelroot elf inspect ./program
```

The non-executing `nelroot elf plan <file>` command validates supported ELF load segments, computes page-aligned virtual ranges and checks entry-point placement under a default W^X policy. On Linux x86_64/AArch64, `nelroot elf map <static-pie>` maps a bounded host-native static PIE into the NELRoot process without executing it. `nelroot elf relocate <static-pie>` additionally applies only supported RELATIVE RELA relocations (x86_64 type 8; AArch64 type 1027), after validating the dynamic table and writable targets. REL, RELR, PLT, symbol resolution and dynamic linking remain unsupported. The `prepare_static_pie` core pipeline now sequences mapping, supported relocations, load planning and initial-stack image construction for host-native static PIE files. The stack remains a separate byte image: it is not mapped into a guest address space, A descriptive architecture-specific initial CPU context (PC/IP and SP) can now be derived and owns the mapped image and stack artifacts; registers are not installed in a real CPU state, the stack is not mapped into the image address space, and no guest code is executed. This is loader preparation, not a complete process loader.

### Download and import an image

Downloads require HTTPS and an expected SHA-256 checksum. NELRoot removes the downloaded file if the checksum does not match.

```bash
nelroot image download https://example.org/rootfs.tar.gz ./rootfs.tar.gz --sha256 <64-hex-digest>
nelroot image import ubuntu ./rootfs.tar.gz --architecture aarch64
nelroot image inspect ubuntu
```

Import currently accepts gzip-compressed tar archives (`.tar.gz` / `.tgz`). Extraction is performed into a unique staging directory and rejects archive paths that would escape that directory. Imported files are stored under `~/.nelroot/images/<name>`. This is image management only: imported root filesystems are not yet booted or executed by NELRoot.


## Build from source

Requirements: stable Rust toolchain and Cargo.

```bash
cargo build --release
cargo test
cargo run -- --help
cargo run -- info
```

## Release automation

Pushing a version tag such as `v0.1.1` triggers GitHub Actions to build platform archives, Debian (`.deb`) and RPM (`.rpm`) packages for supported Linux architectures, generate per-asset SHA-256 checksums, and publish a GitHub Release with the available assets and setup scripts. A manual workflow run can also publish a version tag; see the Actions workflow inputs.

```bash
git tag v0.1.0
git push origin v0.1.0
```

## Architecture goals

- Independent execution engine
- Custom ELF executable loader
- Guest-to-host syscall compatibility layer
- Independent CPU instruction emulator
- Multi-format operating-system image management
- Android support without requiring root, within Android's security and kernel constraints
- Linux, Windows and macOS host support, subject to platform capabilities

## Components

| Component | Responsibility |
|---|---|
| NELRoot CLI | User-facing command interface |
| NELRoot Core | Shared engine coordination |
| NEL Host | Host OS and architecture detection |
| NEL Images | Image catalog and metadata |
| NEL Config | Local configuration |
| NEL ELF | Planned executable loader |
| NEL Runtime | Planned guest execution |
| NEL Syscall | Planned syscall compatibility |
| NEL Emulator | Planned CPU emulator |

## Development principles

- Independent implementation of execution and emulation engines.
- Explicit capability detection; never claim compatibility without tests.
- No root requirement as a design goal on Android, respecting host OS restrictions.
- Modular architecture and documented platform limitations.
- Open development and reproducible tests.

## License

MIT. See [LICENSE](LICENSE).


## Terminal experience

The CLI includes an adaptive terminal UI with a NELRoot banner, colored capability indicators, aligned diagnostic output and styled errors. Colors are enabled only in interactive terminals; set `NO_COLOR=1` for plain output. JSON interfaces remain machine-readable. See [CLI interface documentation](docs/CLI_INTERFACE.md).
