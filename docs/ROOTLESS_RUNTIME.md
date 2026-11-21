# Rootless Linux runtime: implementation contract

This document defines the rootless execution backend and its test contract. A rootless user/mount/PID-namespace launcher now exists with a persistent writable OverlayFS, a supervised guest PID 1 and guest procfs. Networking isolation, complete signal/process lifecycle and distribution compatibility remain incomplete. The guest receives a bounded standard-device set, devpts and a bounded /dev/shm tmpfs.

## Goal

Run same-architecture Linux programs from an imported root filesystem as the invoking user, without requiring host UID 0 and without depending on PRoot, QEMU or Docker. The host Linux kernel remains responsible for native ELF loading and syscall execution in this first native backend.

"Rootless" means the host process does not gain host root privileges. A user namespace may map namespace UID 0 to the invoking unprivileged host UID; that is not host root.

## Required isolation layers

1. **User namespace:** create a new user namespace and map one namespace identity to the caller's host UID/GID. Write and verify `uid_map` and `gid_map`; apply the kernel's setgroups restriction before writing `gid_map` when required.
2. **Mount namespace:** create a private mount namespace so mount propagation cannot affect the host. Never modify host mount propagation.
3. **PID namespace:** create a PID namespace and launch the guest init process as its PID 1. Supervise it from the parent namespace, propagate its exit status and signals, and ensure descendants are cleaned up.
4. **Filesystem root:** validate the rootfs path before entering the namespace, establish the guest root, and ensure all mount operations are scoped to the private mount namespace.
5. **Process credentials:** use namespace credentials only. Do not request host capabilities or retain host-root credentials.
6. **Lifecycle:** own child PID, exit status, signals, cleanup, and failure rollback in a typed runtime session.

Network namespace creation is a separate opt-in milestone. A private network namespace starts isolated and disconnected; NELRoot must not silently expose host interfaces or ports.

## Implementation order

- [x] Read-only policy probe for user-namespace sysctls and visible procfs namespace interfaces.
- [x] Disposable user-namespace creation probe with errno reporting.
- [x] Fail-closed single-ID UID/GID mapping writer and paused-child synchronization protocol.
- [x] Private mount namespace creation and recursive-private propagation.
- [x] Persistent writable OverlayFS per image, preserving the imported rootfs as its immutable lower layer.
- [x] Root transition with `pivot_root` and old-root detachment.
- [x] PID namespace creation with a dedicated guest PID 1 supervised by the parent namespace.
- [x] Guest `/proc` mounted from inside the new PID namespace.
- [x] Linux CLI launcher: `nelroot run-rootless <image> -- <absolute-program> [args...]`.
- [x] Child setup failure reporting, exit-status propagation and runtime mount cleanup.
- [x] Minimal guest `/dev` with bind-mounted `null`, `zero`, `full`, `random` and `urandom` devices.
- [x] Guest devpts instance and bounded `/dev/shm` tmpfs.
- [ ] Additional virtual filesystems and complete signal forwarding/descendant reaping.
- [ ] Resource limits and an explicit security profile.
- [ ] Opt-in network namespace isolation and port forwarding.
- [ ] End-to-end integration tests against minimal Debian, Ubuntu, Alpine and Fedora rootfs images.

## Capability reporting

A visible `/proc/self/ns/user` entry, readable UID/GID maps, or permissive sysctl values are only hints. They do not prove that namespace creation is allowed: seccomp, AppArmor, SELinux, container policy, kernel configuration and namespace quotas can still reject it.

NELRoot now attempts `unshare(CLONE_NEWUSER)` in a disposable forked child and reports the result and errno. This confirms only user-namespace creation; it does not configure UID/GID maps, test mount/PID namespaces, or start a guest process. Do not advertise rootless execution as available until the complete namespace and rootfs startup transaction succeeds.

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


## UID/GID mapping primitive

The Linux runtime exposes `configure_user_namespace_mappings(child_pid)` for the parent side of a future launch protocol. It maps namespace UID/GID 0 to the caller's effective host UID/GID, writes `setgroups=deny` before `gid_map`, and fails closed if any procfs write fails. The child must already have entered its new user namespace and remain blocked while the parent performs these writes.

This is only a mapping primitive: NELRoot does not yet create and synchronize that paused child, so this function is not a complete rootless launcher and is not called by the CLI. Tests cover map formatting without requiring namespace privileges.


## Private mount namespace primitive

On Linux, `prepare_private_mount_namespace()` creates a new mount namespace
for the calling child with `unshare(CLONE_NEWNS)`, then changes propagation
for that namespace's mount tree to recursive-private. It must run after the
child has entered a user namespace and the parent has installed valid UID/GID
maps. Failure is returned immediately; there is no fallback that changes the
host mount namespace.

This primitive does not coordinate the parent/child mapping handshake, mount a
root filesystem, perform `pivot_root`, or launch a guest process. It is a
building block, not a complete runtime capability.


## Synchronized user-to-mount namespace probe

The Linux-only `probe_rootless_user_and_mount_namespaces()` exercises the
parent/child handshake without starting a guest. A disposable child enters a
user namespace and blocks on a synchronization pipe. The parent writes
`setgroups=deny`, `uid_map`, and `gid_map`; only after all writes succeed
does it release the child to create a mount namespace and set recursive-private
propagation. The parent waits for and checks the child's exit status.

This is a diagnostic probe, not the runtime launcher: it exits immediately,
does not mount or pivot into an image rootfs, and does not supervise guest
workloads. The separate PID-namespace probe verifies PID 1 assignment only. Kernel, container, seccomp, or LSM
policy may deny any stage; failures are returned rather than bypassed.


## PID namespace creation probe

The synchronized disposable-child probe also verifies PID namespace setup. After
the mapped child enters a private mount namespace, it calls
`unshare(CLONE_NEWPID)` and forks one final child. That child must observe
`getpid() == 1`, proving that it became PID 1 in the new namespace; the
intermediate child waits for it and propagates failure. This validates namespace
creation and PID-1 assignment only. It does not yet launch an image init,
forward signals, reap arbitrary guest descendants, or provide a complete PID
namespace supervisor.


## Read-only rootfs bind mount

The Linux runtime now exposes `bind_mount_rootfs_read_only(rootfs, target)`
for use inside a mapped user namespace with a private mount namespace already
prepared. It validates the registered rootfs, requires an existing empty real
directory as the destination, rejects overlapping source/target paths, and
creates a bind mount without importing nested submounts. The bind mount is then remounted read-only,
with nosuid and nodev flags. If the remount fails, NELRoot detaches the bind
mount and returns the original error.

This primitive does not create the destination, alter the imported rootfs,
perform `pivot_root`, add writable guest mounts, or launch a process. It must
not be called from the host's ordinary mount namespace; the caller must first
complete namespace mapping and private-mount setup. Rootfs mounting can still
be denied by kernel, container, seccomp, or LSM policy.


## Rootfs entry with pivot_root

The Linux runtime exposes enter_rootfs_with_pivot(rootfs_mount_target) as a
child-only root transition. It requires the caller to have completed UID/GID
mapping, entered a private mount namespace, and mounted the validated rootfs
read-only. The rootfs must contain a real /tmp directory and an existing empty real /run
directory; symlinks and missing directories are rejected.

To avoid modifying the imported image, NELRoot uses the existing empty /run
directory on the rootfs mount as pivot_root's put_old, then detaches the old
root and mounts a small nosuid,nodev tmpfs over guest /tmp. This satisfies
pivot_root's requirement that put_old be on the same mount as new_root.

This function does not create namespaces, perform UID/GID mapping, mount the
rootfs, create /proc or /dev, launch a command, or supervise PID 1. It
must only run in the isolated child. If an error occurs after pivot_root
succeeds, the child must be terminated; it must not continue as though setup
were complete. Kernel or container policy may deny tmpfs mounting or
pivot_root, and NELRoot returns those errors without falling back to chroot.


## First rootless guest launcher

The Linux CLI exposes `nelroot run-rootless <image> -- <absolute-program> [args...]` and `nelroot shell-rootless <image>`. The parent forks a paused child, the child enters a user namespace, and the parent installs a single-ID UID/GID map before releasing it. The child creates a private mount namespace, makes propagation private, mounts a persistent writable OverlayFS over the validated image, creates a PID namespace, and forks the guest init process as PID 1. That process pivots into the guest root, detaches the previous root, mounts procfs for its PID namespace, enables `no_new_privs`, and executes the requested same-architecture program. Standard input, output and error remain connected to the invoking terminal, and the parent supervises PID 1 and returns its exit status.

This path rejects host UID 0, never falls back to chroot, and keeps the imported rootfs unchanged by placing guest writes in a persistent OverlayFS upper layer. The guest process runs as PID 1 inside a new PID namespace, and NELRoot mounts procfs from inside that namespace. A successful launch proves this namespace/rootfs path works on the host; it does not establish general distribution compatibility.

Explicit limitations: the guest `/dev` is intentionally minimal; devices such as `/dev/console` and some distribution-specific device nodes are not provided. Network isolation, resource limits and complete signal/descendant lifecycle handling are not implemented. The host network namespace remains visible. This is not a security boundary against kernel vulnerabilities. Commands needing missing device or service facilities may fail. Kernel, container, seccomp, AppArmor or SELinux policy may deny namespace or mount operations; NELRoot reports the failure without switching to the privileged chroot backend.

Examples:

    nelroot run-rootless alpine -- /bin/sh -c 'printf "rootless OK\\n"'
    nelroot shell-rootless alpine
    nelroot project create demo
    nelroot shell-rootless alpine --workspace ~/.nelroot/projects/demo --cwd /workspace

A supplied `--workspace` must be an existing real host directory. NELRoot bind-mounts
it read/write at guest `/workspace`, with `nosuid` and `nodev` mount flags.
The project files remain on the host and are shared with the guest; this is an
explicit host-filesystem grant, not a general host-root mount. Do not point it at
directories containing data you do not want accessible to guest processes.
