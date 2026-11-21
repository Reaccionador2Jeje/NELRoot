#!/usr/bin/env bash
set -Eeuo pipefail

# Publish an official Alpine Linux Mini Root Filesystem to NELRoot Image Hub.
# Usage: ./scripts/publish-alpine-minirootfs.sh [x86_64|aarch64|armv7|riscv64]
# Optional: ALPINE_VERSION (defaults to 3.24.4), NELROOT_HUB_URL
# Required: NELROOT_HUB_PUBLISH_TOKEN

readonly HUB_URL="${NELROOT_HUB_URL:-https://nelroot-image-hub-production.up.railway.app}"
readonly ALPINE_VERSION="${ALPINE_VERSION:-3.24.4}"
readonly ALPINE_BASE_URL="https://dl-cdn.alpinelinux.org/alpine/latest-stable/releases"

if [[ -z "${NELROOT_HUB_PUBLISH_TOKEN:-}" ]]; then
    echo "ERROR: export NELROOT_HUB_PUBLISH_TOKEN before publishing." >&2
    exit 1
fi
if (( ${#NELROOT_HUB_PUBLISH_TOKEN} < 32 )); then
    echo "ERROR: NELROOT_HUB_PUBLISH_TOKEN must contain at least 32 characters." >&2
    exit 1
fi
if [[ ! "$ALPINE_VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "ERROR: ALPINE_VERSION must look like 3.24.4." >&2
    exit 2
fi

case "${1:-x86_64}" in
    x86_64|amd64)
        readonly ALPINE_ARCH="x86_64"
        readonly NELROOT_ARCH="x86_64"
        ;;
    aarch64|arm64)
        readonly ALPINE_ARCH="aarch64"
        readonly NELROOT_ARCH="aarch64"
        ;;
    armv7)
        readonly ALPINE_ARCH="armv7"
        readonly NELROOT_ARCH="armv7"
        ;;
    riscv64)
        readonly ALPINE_ARCH="riscv64"
        readonly NELROOT_ARCH="riscv64"
        ;;
    *)
        echo "Usage: $0 [x86_64|aarch64|armv7|riscv64]" >&2
        exit 2
        ;;
esac

readonly ARCHIVE_NAME="alpine-minirootfs-${ALPINE_VERSION}-${ALPINE_ARCH}.tar.gz"
readonly RELEASE_URL="${ALPINE_BASE_URL}/${ALPINE_ARCH}"
readonly ARCHIVE_URL="${RELEASE_URL}/${ARCHIVE_NAME}"
readonly WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/nelroot-alpine.XXXXXX")"
readonly MANIFEST_PATH="${WORK_DIR}/manifest.json"
readonly ARCHIVE_PATH="${WORK_DIR}/${ARCHIVE_NAME}"
trap 'rm -rf "$WORK_DIR"' EXIT

for command_name in curl sha256sum awk python3 nelroot; do
    command -v "$command_name" >/dev/null 2>&1 || {
        echo "ERROR: required command not found: $command_name" >&2
        exit 1
    }
done

echo "==> Downloading official Alpine checksum..."
curl --fail --location --silent --show-error --retry 3 \
    --proto '=https' --proto-redir '=https' \
    "${ARCHIVE_URL}.sha256" -o "${WORK_DIR}/archive.sha256"

echo "==> Downloading Alpine Mini Root Filesystem ${ALPINE_VERSION} (${ALPINE_ARCH})..."
curl --fail --location --show-error --retry 3 \
    --proto '=https' --proto-redir '=https' \
    "${ARCHIVE_URL}" -o "${ARCHIVE_PATH}"

EXPECTED_SHA256="$(awk 'NR == 1 { print $1; exit }' "${WORK_DIR}/archive.sha256")"
if [[ ! "$EXPECTED_SHA256" =~ ^[[:xdigit:]]{64}$ ]]; then
    echo "ERROR: Alpine checksum file did not contain a valid SHA-256 digest." >&2
    exit 1
fi

printf '%s  %s\n' "$EXPECTED_SHA256" "$ARCHIVE_PATH" | sha256sum --check --status
echo "==> Official archive checksum verified: $EXPECTED_SHA256"

export HUB_ARCHIVE_PATH="$ARCHIVE_PATH"
export HUB_ARCHIVE_SHA256="$EXPECTED_SHA256"
export HUB_ARCHIVE_URL="$ARCHIVE_URL"
export HUB_MANIFEST_PATH="$MANIFEST_PATH"
export HUB_NELROOT_ARCH="$NELROOT_ARCH"
export HUB_ALPINE_VERSION="$ALPINE_VERSION"

python3 - <<'PY'
import json
import os
from pathlib import Path

archive = Path(os.environ["HUB_ARCHIVE_PATH"])
version = os.environ["HUB_ALPINE_VERSION"]
architecture = os.environ["HUB_NELROOT_ARCH"]
manifest = {
    "schema_version": 1,
    "image": {
        "id": f"atdev/alpine-{version}",
        "name": "Alpine Linux Mini Root Filesystem",
        "version": version,
        "distribution": "Alpine Linux",
        "release": version,
        "architecture": architecture,
        "description": "Official minimal Alpine Linux root filesystem for NELRoot; not an installation ISO.",
        "maintainer": "AtDev",
        "license": "Mixed",
        "tags": ["alpine", "linux", "minirootfs", "minimal", version],
        "rootfs": {
            "format": "tar.gz",
            "size_bytes": archive.stat().st_size,
            "sha256": os.environ["HUB_ARCHIVE_SHA256"],
            "download_url": os.environ["HUB_ARCHIVE_URL"],
        },
    },
}
Path(os.environ["HUB_MANIFEST_PATH"]).write_text(
    json.dumps(manifest, indent=2) + "\n", encoding="utf-8"
)
PY

echo "==> Validating NELRoot manifest..."
nelroot image hub-manifest-validate "$MANIFEST_PATH"

echo "==> Publishing Alpine Linux to the first-party NELRoot Image Hub..."
nelroot image hub-publish "$MANIFEST_PATH" "$ARCHIVE_PATH" --hub "$HUB_URL"

echo "==> Confirming the image is visible in the Hub catalog..."
nelroot image hub-remote-search alpine --hub "$HUB_URL" --architecture "$NELROOT_ARCH"

echo "==> Done. Pull it with:"
echo "nelroot image hub-pull atdev/alpine-$ALPINE_VERSION --hub $HUB_URL --architecture $NELROOT_ARCH"
