#!/usr/bin/env bash
set -euo pipefail

NELROOT_BIN="${NELROOT_BIN:-target/release/nelroot}"
BUSYBOX_BIN="${BUSYBOX_BIN:-/bin/busybox}"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

if [[ ! -x "$NELROOT_BIN" ]]; then
  echo "NELROOT_BIN is not executable: $NELROOT_BIN" >&2
  exit 1
fi
if [[ ! -x "$BUSYBOX_BIN" ]]; then
  echo "busybox-static is required at $BUSYBOX_BIN" >&2
  exit 1
fi

export HOME="$WORK/home"
mkdir -p "$HOME" "$WORK/rootfs"/{bin,etc,proc,run,tmp,dev/pts,dev/shm,workspace,usr/bin}
chmod 1777 "$WORK/rootfs/tmp" "$WORK/rootfs/dev/shm"
install -m 0755 "$BUSYBOX_BIN" "$WORK/rootfs/bin/busybox"
for applet in sh uname env sleep; do
  ln -s busybox "$WORK/rootfs/bin/$applet"
done
cat > "$WORK/rootfs/etc/os-release" <<'EOF'
NAME="NELRoot Rootless Integration"
ID=nelroot-smoke
EOF

ARCH="$(uname -m)"
"$NELROOT_BIN" image register rootless-smoke "$WORK/rootfs" --architecture "$ARCH"

OUTPUT="$("$NELROOT_BIN" run-rootless rootless-smoke -- /bin/sh -c 'printf "ROOTLESS_SMOKE_OK\\n"; /bin/uname -m; /bin/sh -c "sleep 1 & exit 0"')"
printf '%s\n' "$OUTPUT"
grep -q '^ROOTLESS_SMOKE_OK$' <<<"$OUTPUT"
grep -q "^$ARCH$" <<<"$OUTPUT"
echo "Rootless runtime integration smoke test passed."
