# NELRoot

**Independent Linux Execution & Virtualization Engine**

NELRoot is an open-source systems project by **AtDev**. Its long-term goal is to execute Linux environments and other system images through independently developed runtime, executable-loading, syscall-compatibility, and CPU-emulation components.

NELRoot is designed as an independent engine. PRoot and QEMU are not execution backends or runtime requirements.

> **Status:** Early development. The current code is project scaffolding and host diagnostics; it does not yet execute guest Linux programs or boot virtual machines.

## Project goals

- Native host and architecture detection
- Operating-system image catalog and lifecycle management
- Custom ELF executable loader
- Independent Linux userspace execution runtime
- Guest-to-host syscall compatibility layer
- CPU instruction emulation for supported architectures
- Android support without requiring root, within Android's security and kernel constraints
- Linux and Windows host support, subject to platform capabilities

## Architecture

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

## Build

Requirements: Rust toolchain (stable) and Cargo.

```bash
cargo build
cargo test
cargo run -- info
cargo run -- doctor
cargo run -- image list
cargo run -- engine status
cargo run -- init
```

## Development principles

- Independent implementation of the execution and emulation engines.
- Explicit capability detection; never claim compatibility without tests.
- No root requirement as a design goal on Android, while respecting host OS restrictions.
- Modular architecture and documented platform limitations.
- Open development and reproducible tests.

## License

MIT. See [LICENSE](LICENSE).
