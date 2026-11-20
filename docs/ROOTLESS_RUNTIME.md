# Rootless Linux runtime: implementation contract

This document defines the next execution backend. It is a design and test contract; it does not claim that rootless execution is implemented yet.

## Goal

Run same-architecture Linux programs from an imported root filesystem as the invoking user, without requiring host UID 0 and without depending on PRoot, QEMU or Docker. The host Linux kernel remains responsible for native ELF loading and syscall execution in this first native backend.

"Rootless" means the host process does not gain host root privileges. A user namespace may map namespace UID 0 to the invoking unprivileged host UID; that is not host root.

## Required isolation layers

1. **User namespace:** create a new user namespace and map one namespace identity to the caller's host UID/GID. Write and verify `uid_map` and `gid_map`; apply the kernel's setgroups restriction before writing `gid_map` when required.
2. **Mount namespace:** create a private mount namespace so mount propagation cannot affect the host. Never modify host mount propagation.
3. **PID namespace:** create a PID namespace and launch the guest init process as its PID 1. Supervise it from the parent namespace and reap children.
4. **Filesystem root:** validate the rootfs path before entering the namespace, establish the guest root, and ensure all mount operations are scoped to the private mount namespace.
5. **Process credentials:** use namespace credentials only. Do not request host capabilities or retain host-root credentials.
6. **Lifecycle:** own child PID, exit status, signals, cleanup, and failure rollback in a typed runtime session.

Network namespace creation is a separate opt-in milestone. A private network namespace starts isolated and disconnected; NELRoot must not silently expose host interfaces or ports.

## Implementation order

- [x] Read-only policy probe for user namespace sysctls and visible procfs namespace interfaces.
- [ ] Add a small Linux-only namespace backend with typed errors and explicit feature flags.
- [ ] Create and configure user namespace mappings with strict parent/child synchronization.
- [ ] Create private mount namespace and make propagation private.
- [x] Add an owned child-process supervisor with polling, wait, exit-status propagation and best-effort cleanup (process primitive only; no namespaces or guest root setup).
- [ ] Create and configure user namespace mappings with strict parent/child synchronization.
- [ ] Enter the validated rootfs and start a supervised child process.
- [ ] Add PID namespace supervision and signal/exit propagation.
- [ ] Add optional read-only `/proc`, minimal `/dev`, and other guest mounts only after mount isolation is proven.
- [ ] Add resource limits and an explicit security profile.
- [ ] Add opt-in networking and port forwarding.
- [ ] Run integration tests against minimal Debian, Ubuntu, Alpine and Fedora rootfs images.

## Capability reporting

A visible `/proc/self/ns/user` entry, readable UID/GID maps, or permissive sysctl values are only hints. They do not prove that namespace creation is allowed: seccomp, AppArmor, SELinux, container policy, kernel configuration and namespace quotas can still reject it.

The actual backend must attempt namespace creation in a disposable child and report the kernel error without changing the parent process. Do not advertise rootless execution as available until that end-to-end probe succeeds.

## Safety and correctness rules

- Never fall back to chroot-only execution while reporting rootless isolation.
- Never interpret UID 0 inside a user namespace as host root.
- Never write global sysctls or change host mount propagation.
- Never mount or expose host paths unless explicitly configured.
- Reject path traversal and unsafe rootfs symlinks during root setup.
- Do not execute untrusted images with host credentials.
- Treat namespace creation, mount setup, and process launch as one rollback-capable transaction.
- Keep cross-architecture execution separate; this backend is native same-architecture only.

## Distribution test gate

A distribution is not "supported" because its rootfs imports successfully. The minimum smoke suite must launch real guest binaries and verify:

- `/bin/sh -c 'printf ...'` works.
- `/bin/uname -m` reports the guest's native architecture.
- `/usr/bin/env` sees only the configured guest environment.
- `/etc/os-release` identifies the expected distribution.
- process exit codes and signals propagate correctly.
- guest processes cannot see host paths outside explicitly granted mounts.
- cleanup leaves no mount or process behind.

Package-manager tests (`apt`, `apk`, `dnf`) come only after the basic process, filesystem, networking and certificate-store requirements are met.
