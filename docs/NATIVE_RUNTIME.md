# Native runtime: first execution backend

NELRoot now includes a native Linux process launcher for registered, same-architecture root filesystems.

## Commands

```bash
# Inspect runtime and host capabilities
nelroot doctor
nelroot engine status

# Run one native executable inside an image
nelroot run ubuntu -- /bin/uname -a
nelroot run ubuntu --cwd /workspace -- /usr/bin/env

# Start an interactive shell
nelroot shell ubuntu
nelroot shell ubuntu --shell /bin/bash --cwd /workspace

# For a trusted image that genuinely needs guest root
nelroot run ubuntu --keep-root -- /usr/bin/id
```

## How it works

On Linux hosts, NELRoot starts a child process, calls the kernel's `chroot(2)` in the child, changes to the requested guest working directory, applies the requested credentials, and then executes the absolute guest program path. The host Linux kernel loads the guest ELF and services its syscalls.

This backend:
- Does not invoke PRoot, QEMU or Docker.
- Supports native same-architecture images only.
- Requires effective UID 0 and effective `CAP_SYS_CHROOT`, `CAP_SETUID` and `CAP_SETGID` capabilities for the default credential-dropping mode.
- Drops supplementary groups and changes the child to UID/GID 65534 by default.
- Sets `no_new_privs` after dropping credentials.
- Uses a minimal environment and inherits the terminal's standard input/output/error.
- Preserves the child exit code.

Ordinary unprivileged PRoot sessions and typical Android/Termux installations cannot use this backend. A root-looking UID inside PRoot does not grant these host capabilities. NELRoot checks the effective capability mask in `/proc/self/status` rather than trusting the UID alone.

## Important security boundary

**chroot is filesystem path isolation, not a secure container or a complete sandbox.** This initial backend does not create mount, PID, user or network namespaces; does not mount /proc, /sys or /dev; and does not filter network access. The default unprivileged guest identity reduces risk but does not turn chroot into a security boundary.

`--keep-root` is explicitly unsafe: the guest process retains host root credentials. Use it only with images and executables you fully trust. Do not use this mode for untrusted images.

## What this is not

This is real process execution from a Linux rootfs, but it is not yet NELRoot's independent ELF loader or syscall implementation. The host kernel performs ELF loading and syscall handling. Cross-architecture emulation, rootless user namespaces, mount management, process supervision, a virtual filesystem, and the independent ABI/syscall engine remain future milestones.

On unsupported host operating systems, the commands fail clearly rather than silently falling back to another runtime.
