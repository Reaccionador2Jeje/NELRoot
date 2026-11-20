# NELRoot

**Independent Linux Execution & Virtualization Engine**

NELRoot is an open-source systems project by **AtDev**. Its long-term goal is to execute Linux environments and other system images through independently developed runtime, executable-loading, syscall-compatibility, and CPU-emulation components.

NELRoot is designed as an independent engine. PRoot and QEMU are not execution backends or runtime requirements.

> **Development status:** Early development. The current release provides the CLI, host diagnostics, configuration and image-catalog scaffolding. Guest Linux execution, ELF loading and CPU emulation are not implemented yet.

## Install

Download the latest installer from the [GitHub Releases](https://github.com/Reaccionador2Jeje/NELRoot/releases/latest) page.

### Linux, macOS and Android (Termux)

Run the setup wizard:

```bash
python3 -c "import urllib.request; open('nelroot-install.sh','wb').write(urllib.request.urlopen('https://raw.githubusercontent.com/Reaccionador2Jeje/NELRoot/main/scripts/install.sh').read())"
bash nelroot-install.sh
```

The wizard detects the operating system and CPU architecture, downloads the matching release through Python's standard HTTPS library, verifies its SHA-256 checksum, asks where to install, and configures the user PATH. It does not call curl or wget. Python 3 and tar are required on Unix-like systems. Choose system-wide installation when prompted if you want the executable available to all local users; this requires sudo permissions. On Termux, install Python and tar with `pkg install python tar`, then run the wizard; it installs into the Termux prefix without root.

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
nelroot image inspect <name>

nelroot engine status
```

Run `nelroot <command> --help` for command-specific help.

## Build from source

Requirements: stable Rust toolchain and Cargo.

```bash
cargo build --release
cargo test
cargo run -- --help
cargo run -- info
```

## Release automation

Pushing a version tag such as `v0.1.0` triggers GitHub Actions to build the platform packages, generate SHA-256 checksum files, and publish a GitHub Release with the archives and setup scripts.

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
