#!/usr/bin/env bash
set -euo pipefail

IMAGE="${1:?usage: smoke-test-distro-rootfs.sh IMAGE LOCAL_NAME}"
LOCAL_NAME="${2:?usage: smoke-test-distro-rootfs.sh IMAGE LOCAL_NAME}"
NELROOT_BIN="${NELROOT_BIN:-target/release/nelroot}"
WORK="$(mktemp -d)"
CONTAINER_ID=""
cleanup() {
  if [[ -n "$CONTAINER_ID" ]]; then docker rm -f "$CONTAINER_ID" >/dev/null 2>&1 || true; fi
  rm -rf "$WORK"
}
trap cleanup EXIT

docker pull "$IMAGE"
CONTAINER_ID="$(docker create "$IMAGE" /bin/sh -c 'exit 0')"
ROOTFS="$WORK/rootfs"
mkdir -p "$ROOTFS"
docker export "$CONTAINER_ID" | tar -xf - -C "$ROOTFS"
docker rm "$CONTAINER_ID" >/dev/null
CONTAINER_ID=""


# A distro image may ship transient runtime files; the pivot_root put_old
# directory must be a dedicated empty mountpoint in this test rootfs.
rm -rf "$ROOTFS/run"
mkdir -p "$ROOTFS/run" "$ROOTFS/tmp" "$ROOTFS/proc" "$ROOTFS/dev/pts" "$ROOTFS/dev/shm" "$ROOTFS/workspace"
chmod 1777 "$ROOTFS/tmp" "$ROOTFS/dev/shm"

export HOME="$WORK/home"
mkdir -p "$HOME"
ARCH="$(uname -m)"
"$NELROOT_BIN" image register "$LOCAL_NAME" "$ROOTFS" --architecture "$ARCH"
OUTPUT="$("$NELROOT_BIN" run-rootless "$LOCAL_NAME" -- /bin/sh -c 'printf "DISTRO_ROOTFS_OK\\n"; uname -m; printf "DISTRO_ID="; . /etc/os-release; printf "%s\\n" "${ID:-unknown}"')"
printf '%s\n' "$OUTPUT"
grep -q '^DISTRO_ROOTFS_OK$' <<<"$OUTPUT"
grep -q "^$ARCH$" <<<"$OUTPUT"
grep -q '^DISTRO_ID=' <<<"$OUTPUT"
echo "Rootfs smoke test passed: $IMAGE"
