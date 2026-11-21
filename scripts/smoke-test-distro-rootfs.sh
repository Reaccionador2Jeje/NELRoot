#!/usr/bin/env bash
set -euo pipefail

IMAGE_ID="${1:?usage: smoke-test-distro-rootfs.sh IMAGE_ID LOCAL_NAME EXPECTED_DISTRO_ID}"
LOCAL_NAME="${2:?usage: smoke-test-distro-rootfs.sh IMAGE_ID LOCAL_NAME EXPECTED_DISTRO_ID}"
EXPECTED_DISTRO_ID="${3:?usage: smoke-test-distro-rootfs.sh IMAGE_ID LOCAL_NAME EXPECTED_DISTRO_ID}"
NELROOT_BIN="${NELROOT_BIN:-target/release/nelroot}"
HUB_URL="${NELROOT_HUB_URL:-https://nelroot-image-hub-production.up.railway.app}"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

write_summary() {
  if [[ -n "${GITHUB_STEP_SUMMARY:-}" ]]; then
    printf "%s\\n" "$1" >> "$GITHUB_STEP_SUMMARY"
  fi
}

for command_name in curl python3; do
  command -v "$command_name" >/dev/null 2>&1 || {
    echo "Required command not found: $command_name" >&2
    exit 1
  }
done
[[ -x "$NELROOT_BIN" ]] || { echo "NELROOT_BIN is not executable: $NELROOT_BIN" >&2; exit 1; }

ARCH="$(uname -m)"
echo "Checking first-party NELRoot Image Hub for $IMAGE_ID ($ARCH)..."
if ! curl --fail --location --silent --show-error --retry 2 \
    --proto '=https' --proto-redir '=https' \
    "$HUB_URL/v1/catalog" -o "$WORK/catalog.json"; then
  echo "ERROR: could not read the NELRoot Image Hub catalog." >&2
  exit 1
fi

if ! python3 - "$WORK/catalog.json" "$IMAGE_ID" "$ARCH" <<'PY'
import json
import sys
with open(sys.argv[1], encoding="utf-8") as stream:
    catalog = json.load(stream)
found = any(
    entry.get("id") == sys.argv[2]
    and entry.get("architecture") == sys.argv[3]
    for entry in catalog.get("images", [])
)
sys.exit(0 if found else 1)
PY
then
  echo "SKIP: $IMAGE_ID ($ARCH) is not published in the first-party NELRoot Image Hub catalog yet."
  write_summary "### NELRoot Image Hub: $IMAGE_ID\\n\\n| Architecture | Result |\\n|---|---|\\n| $ARCH | **SKIP — not published in the Hub catalog** |"
  exit 0
fi

export HOME="$WORK/home"
mkdir -p "$HOME"
# hub-pull verifies the catalog, manifest, archive size and SHA-256 before
# extracting and registering the rootfs. No Docker daemon or Docker Hub used.
"$NELROOT_BIN" image hub-pull "$IMAGE_ID" --hub "$HUB_URL" --architecture "$ARCH" --name "$LOCAL_NAME"
write_summary "### NELRoot Image Hub: $IMAGE_ID\\n\\n| Architecture | Hub pull | Guest execution |\\n|---|---|---|\\n| $ARCH | **PASS — verified and registered** | Pending |"

if OUTPUT="$("$NELROOT_BIN" run-rootless "$LOCAL_NAME" -- /bin/sh -c 'printf "NELROOT_HUB_ROOTFS_OK\\n"; uname -m; printf "DISTRO_ID="; . /etc/os-release; printf "%s\\n" "${ID:-unknown}"' 2>&1)"; then
  printf '%s\n' "$OUTPUT"
  grep -q '^NELROOT_HUB_ROOTFS_OK$' <<<"$OUTPUT"
  grep -q "^$ARCH$" <<<"$OUTPUT"
  grep -Fxq "DISTRO_ID=$EXPECTED_DISTRO_ID" <<<"$OUTPUT"
  echo "First-party Hub rootfs smoke test passed: $IMAGE_ID"
  write_summary "### NELRoot Image Hub: $IMAGE_ID\\n\\n| Architecture | Hub pull | Guest execution |\\n|---|---|---|\\n| $ARCH | **PASS — verified and registered** | **PASS** |"
else
  STATUS=$?
  if grep -Fq "rootless runtime failed to create private mount namespace: Operation not permitted" <<<"$OUTPUT"; then
    echo "SKIP: runner policy blocks rootless mount namespaces; verified Hub pull and image registration passed for $IMAGE_ID."
    write_summary "### NELRoot Image Hub: $IMAGE_ID\\n\\n| Architecture | Hub pull | Guest execution |\\n|---|---|---|\\n| $ARCH | **PASS — verified and registered** | **SKIP — runner blocks private mount namespaces** |"
    exit 0
  fi
  printf '%s\n' "$OUTPUT" >&2
  exit "$STATUS"
fi
