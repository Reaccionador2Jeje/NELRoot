#!/usr/bin/env bash
set -euo pipefail

NELROOT_BIN="${NELROOT_BIN:-target/release/nelroot}"
BUSYBOX_BIN="${BUSYBOX_BIN:-/bin/busybox}"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

write_summary() {
  if [[ -n "${GITHUB_STEP_SUMMARY:-}" ]]; then
    printf '%s\n' "$@" >> "$GITHUB_STEP_SUMMARY"
  fi
}

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

# A restricted hosted runner may prohibit creation of a private mount namespace.
if OUTPUT="$("$NELROOT_BIN" run-rootless rootless-smoke -- /bin/sh -c 'printf "ROOTLESS_SMOKE_OK\\n"; /bin/uname -m; /bin/sh -c "sleep 1 & exit 0"' 2>&1)"; then
  printf '%s\n' "$OUTPUT"
  grep -q '^ROOTLESS_SMOKE_OK$' <<<"$OUTPUT"
  grep -q "^$ARCH$" <<<"$OUTPUT"
else
  STATUS=$?
  if grep -Fq "rootless runtime failed to create private mount namespace: Operation not permitted" <<<"$OUTPUT"; then
    echo "SKIP: CI runner policy blocks rootless mount namespaces; NELRoot native rootfs registration passed."
    write_summary "### NELRoot native rootfs smoke" "" "**PASS:** native rootfs registered." "**SKIP:** guest launch blocked by runner mount-namespace policy."
    exit 0
  fi
  printf '%s\n' "$OUTPUT" >&2
  exit "$STATUS"
fi

# Verify the OverlayFS upper layer survives a second independent guest launch.
"$NELROOT_BIN" run-rootless rootless-smoke -- /bin/sh -c 'printf "NELROOT_OVERLAY_PERSISTED\\n" > /tmp/nelroot-overlay-persistence'
PERSIST_OUTPUT="$("$NELROOT_BIN" run-rootless rootless-smoke -- /bin/sh -c 'test -f /tmp/nelroot-overlay-persistence && printf "NELROOT_OVERLAY_PERSISTED\\n"')"
grep -q '^NELROOT_OVERLAY_PERSISTED$' <<<"$PERSIST_OUTPUT"

# Verify that the guest's exit status is propagated by the host CLI.
if "$NELROOT_BIN" run-rootless rootless-smoke -- /bin/sh -c 'exit 23'; then
  echo "Expected guest exit code 23, but the command returned success." >&2
  exit 1
else
  GUEST_STATUS=$?
  if [[ "$GUEST_STATUS" != "23" ]]; then
    echo "Expected guest exit code 23, received $GUEST_STATUS." >&2
    exit 1
  fi
fi

echo "Rootless runtime integration smoke test passed (launch, architecture, persistent OverlayFS and exit status)."
write_summary "### NELRoot native rootfs smoke" "" "**PASS:** native rootfs registered; guest launch, architecture, persistent OverlayFS and exit-status propagation verified."
