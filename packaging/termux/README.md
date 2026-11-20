# Termux package integration

Termux packages are built inside the Termux packaging environment and published through a Termux-compatible repository. A normal Android NDK binary archive is not automatically a Termux package.

## Planned recipe values

- Package name: `nelroot`
- Version: track the matching NELRoot release tag.
- Architecture: `aarch64` initially.
- Build: Rust stable toolchain for the Termux host target.
- Install: `$PREFIX/bin/nelroot`.
- Data: `$HOME/.nelroot`; package removal must preserve this directory.
- Runtime dependencies: none beyond Android/Termux system libraries used by the binary.

The recipe must be validated with the current Termux package build system before publication. Do not add PRoot, QEMU or Docker as dependencies.
