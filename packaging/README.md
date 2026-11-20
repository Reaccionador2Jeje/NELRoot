# Native package distribution

This directory tracks packaging work for AtDev-maintained distribution channels.

## Target formats

| Ecosystem | Format | Planned build environment |
|---|---|---|
| Debian / Ubuntu | .deb + APT metadata | Debian/Ubuntu with dpkg-deb |
| Fedora / RHEL / openSUSE | .rpm + DNF/Zypper metadata | RPM build environment |
| Arch Linux | .pkg.tar.zst + Pacman database | Arch Linux with makepkg |
| Alpine Linux | .apk + APKINDEX | Alpine with abuild |
| Termux | Termux package | Termux packaging environment |

## Packaging policy

- Package the NELRoot executable and documentation only; do not bundle a guest distribution.
- Preserve user data under `~/.nelroot` and never delete it on package removal.
- Do not run the application as root by default.
- Do not add PRoot, QEMU or Docker as runtime dependencies.
- Build each package for its actual target architecture.
- Publish checksums, provenance and signing instructions.
- Test fresh install, upgrade, uninstall and reinstall in clean target environments.

## Status

The release workflow currently produces portable archives for Linux x86_64 and ARM64, Android ARM64, Windows x86_64 and macOS Intel/Apple Silicon. Initial Debian, RPM, Arch and Alpine build definitions are now present. The Debian package is built in the release workflow and smoke-tested in CI; the other recipes remain templates until their build environments and checksums are validated. Native repository signing, repository publication, Termux package recipe integration and clean-distro install tests remain unfinished. Do not advertise APT/DNF/Pacman/APK repositories until those steps are implemented and tested.
