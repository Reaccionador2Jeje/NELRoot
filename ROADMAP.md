# NELRoot Roadmap

NELRoot's direction is to become a modern, friendly, independent Linux environment for developers and tools. It must not quietly fall back to PRoot, QEMU, or Docker as its execution engine. Compatibility and platform limits must be explicit and tested.

This roadmap describes goals, not claims that the features are already implemented.

## 1. Core execution engine

- [x] CLI foundation, host diagnostics, configuration and local image catalog.
- [x] Validate, register, inspect and unregister existing root filesystem directories.
- [x] HTTPS image archive download with mandatory SHA-256 verification.
- [x] Safe gzip/tar rootfs import into NELRoot-managed image storage.
- [x] Versioned HTTPS image registry index with search, architecture selection, verified pull and safe stale-cache cleanup.
- [x] User-scoped data/cache/log/runtime directory initialization and local event log.
- [x] Expanded host diagnostics with kernel and host-interface detection.
- [ ] Support additional archive formats and distribution-specific image manifests.
- [ ] Publish and operate an official AtDev image registry with maintained Ubuntu, Debian and Alpine manifests.
- [ ] Independent NELRoot runtime and process lifecycle.
- [ ] ELF64 inspection/parser module (first bounded runtime milestone).
- [ ] ELF loader and dynamic-linking support.
- [ ] Linux syscall compatibility layer.
- [ ] Filesystem, process, signal, networking and namespace compatibility where host platforms permit.
- [ ] Resource limits, isolation profiles, permissions and clear security boundaries.
- [ ] Reliable cleanup, diagnostics, logs and recovery after interrupted sessions.

## 2. Developer environment: a friendly alternative

NELRoot should make it straightforward for a developer to use a Linux environment without needing to learn or operate PRoot, QEMU, or Docker.

Target workflows:

- Open an interactive Linux shell.
- Run common Linux development commands and toolchains.
- Clone, build, test and compile projects.
- Run local web servers, APIs, databases and development services where supported.
- Expose and manage development ports safely.
- Package projects into distributable artifacts.
- Manage environment variables, working directories, processes and persistent project files.
- Create, start, stop, inspect, export and remove named environments through simple commands.
- Provide clear errors, sensible defaults, progress feedback and guided setup.

Proposed future command experience (illustrative, not implemented yet):

```bash
nelroot create ubuntu
nelroot start ubuntu
nelroot shell ubuntu
nelroot run ubuntu -- apt update
nelroot run ubuntu -- npm run dev
nelroot ps
nelroot stop ubuntu
nelroot package my-project
```

The actual command syntax will be finalized as the runtime and security model are implemented.

## 3. NEXORA integration: replace the PRoot backend

A key product objective is to replace NEXORA's current PRoot-based Ubuntu execution backend with NELRoot.

Integration goals:

- NEXORA uses NELRoot as its Linux environment/runtime provider.
- Existing Ubuntu workflows are migrated deliberately, with feature parity assessed before switching defaults.
- NEXORA can create, start, stop, enter and inspect NELRoot environments through a stable interface.
- Project folders, persistent data, environment variables, process output, exit codes and port forwarding have documented contracts.
- NEXORA's terminal and application UI report runtime state and actionable errors.
- The migration includes compatibility tests for package installation, development commands, web servers and project builds.
- A migration/rollback path is maintained until NELRoot is proven reliable for NEXORA's supported workloads.

The long-term product direction is for NEXORA users and developers to choose NELRoot rather than having to configure PRoot, QEMU or Docker. NELRoot must provide its own implementation; it must not disguise one of those tools as its backend.

## 4. Native Linux package distribution

NELRoot should be installable using the package manager users already know. The initial distribution strategy is to publish signed packages in AtDev-maintained repositories, then pursue inclusion in official distribution repositories where maintainers and policies allow it.

| Ecosystem | Package format | Distribution goal |
|---|---|---|
| Debian / Ubuntu | `.deb`, APT repository | Signed APT repository with Release metadata |
| Fedora / RHEL / openSUSE | `.rpm`, DNF/Zypper repository | Signed RPM repository metadata |
| Arch Linux | `.pkg.tar.zst` | Pacman repository with database and signatures |
| Alpine Linux | `.apk` | APK repository and index |
| Termux | Termux package | Termux-compatible package recipe/repository |
| Other Linux distributions | Portable archive / future native package | Documented fallback and additional package formats |

Repository and packaging requirements:

- Build packages from tagged, reproducible release sources.
- Publish architecture-specific packages (starting with x86_64 and AArch64 where supported).
- Sign repository metadata and packages; publish verification instructions and signing-key fingerprints.
- Generate checksums and software provenance/SBOM information.
- Maintain versioned repositories and a documented update policy.
- Test install, upgrade, downgrade where supported, uninstall and clean reinstall.
- Keep packages unprivileged by default; request elevated permissions only for explicitly chosen system-level operations.
- Avoid requiring curl or wget in the NELRoot installer or package bootstrap path.
- Clearly distinguish AtDev-hosted repositories from packages accepted into official distribution repositories.

Official repository inclusion is a separate contribution and review process. Until accepted, installation will use AtDev-maintained repositories or release packages.

## 5. Release engineering and quality

- [ ] Validate every supported release target in GitHub Actions.
- [ ] Publish first tested GitHub Release with checksums.
- [ ] Add automated .deb, .rpm, Pacman and APK packaging.
- [ ] Validate package installation, upgrade and uninstall on clean distribution images.
- [ ] Add signed repository publishing and key-rotation documentation.
- [ ] Run integration tests against clean Ubuntu, Debian, Fedora and Arch environments.
- [ ] Publish support matrix, known limitations and upgrade guidance.

## 6. Runtime architecture preparation

The independent runtime design and first implementation milestone are documented in [docs/RUNTIME_ARCHITECTURE.md](docs/RUNTIME_ARCHITECTURE.md). The first code milestone is an ELF64 inspection/parser that does not execute guest code.

## 7. NEXORA runtime interface preparation

The runtime integration contract is documented in [docs/NEXORA_INTEGRATION.md](docs/NEXORA_INTEGRATION.md). NEXORA must call a stable NELRoot interface and must not assume that image registration means an environment can already execute.

## 8. Product principles

1. **Independent runtime:** NELRoot is the engine, not a wrapper around PRoot, QEMU or Docker.
2. **Developer-friendly:** common tasks should be simple, discoverable and consistent.
3. **Honest compatibility:** advertise only capabilities that pass automated tests.
4. **Safe by design:** isolation boundaries, permissions and host access are explicit.
5. **Portable distribution:** native package managers first, portable releases as a fallback.
6. **Open development:** documented interfaces, reproducible builds and transparent limitations.

## Current reality

NELRoot is still in early development. The current CLI and image catalog do not yet boot a Linux guest, provide a full Linux shell, run Linux services, or package arbitrary projects. Those capabilities depend on the independent runtime, loader, syscall and networking work above.
