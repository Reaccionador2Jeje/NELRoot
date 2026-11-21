#!/usr/bin/env bash
set -Eeuo pipefail

# Publish an official Ubuntu Base rootfs to the first-party NELRoot Image Hub.
# Usage: ./scripts/publish-ubuntu-base.sh [amd64|arm64]
# Required: curl, sha256sum, python3, nelroot, NELROOT_HUB_PUBLISH_TOKEN

readonly HUB_URL="${NELROOT_HUB_URL:-https://nelroot-image-hub-production.up.railway.app}"
readonly UBUNTU_RELEASE="24.04"
readonly UBUNTU_POINT_RELEASE="24.04.5"
readonly UBUNTU_BASE_URL="https://cdimage.ubuntu.com/ubuntu-base/releases/${UBUNTU_RELEASE}/release"

if [[ -z "${NELROOT_HUB_PUBLISH_TOKEN:-}" ]]; then
    echo "ERROR: export NELROOT_HUB_PUBLISH_TOKEN before publishing." >&2
    exit 1
fi
if (( ${#NELROOT_HUB_PUBLISH_TOKEN} < 32 )); then
    echo "ERROR: NELROOT_HUB_PUBLISH_TOKEN must contain at least 32 characters." >&2
    exit 1
fi

case "${1:-amd64}" in
    amd64|x86_64)
        readonly UBUNTU_ARCH="amd64"
        readonly NELROOT_ARCH="x86_64"
        ;;
    arm64|aarch64)
        readonly UBUNTU_ARCH="arm64"
        readonly NELROOT_ARCH="aarch64"
        ;;
    armhf|arm)
        readonly UBUNTU_ARCH="armhf"
        readonly NELROOT_ARCH="arm"
        ;;
    ppc64el|ppc64le)
        readonly UBUNTU_ARCH="ppc64el"
        readonly NELROOT_ARCH="ppc64le"
        ;;
    riscv64)
        readonly UBUNTU_ARCH="riscv64"
        readonly NELROOT_ARCH="riscv64"
        ;;
    s390x)
        readonly UBUNTU_ARCH="s390x"
        readonly NELROOT_ARCH="s390x"
        ;;
    *)
        echo "Usage: $0 [amd64|arm64|armhf|ppc64el|riscv64|s390x]" >&2
        exit 2
        ;;
esac

readonly ARCHIVE_NAME="ubuntu-base-${UBUNTU_POINT_RELEASE}-base-${UBUNTU_ARCH}.tar.gz"
readonly ARCHIVE_URL="${UBUNTU_BASE_URL}/${ARCHIVE_NAME}"
readonly WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/nelroot-ubuntu-base.XXXXXX")"
readonly MANIFEST_PATH="${WORK_DIR}/manifest.json"
readonly ARCHIVE_PATH="${WORK_DIR}/${ARCHIVE_NAME}"
trap 'rm -rf "$WORK_DIR"' EXIT

for command_name in curl sha256sum python3 nelroot; do
    command -v "$command_name" >/dev/null 2>&1 || {
        echo "ERROR: required command not found: $command_name" >&2
        exit 1
    }
done

echo "==> Downloading official Ubuntu Base ${UBUNTU_POINT_RELEASE} checksums..."
curl --fail --location --silent --show-error --retry 3 \
    --proto '=https' --proto-redir '=https' \
    "${UBUNTU_BASE_URL}/SHA256SUMS" -o "${WORK_DIR}/SHA256SUMS"

echo "==> Downloading ${ARCHIVE_NAME}..."
curl --fail --location --show-error --retry 3 \
    --proto '=https' --proto-redir '=https' \
    "${ARCHIVE_URL}" -o "${ARCHIVE_PATH}"

EXPECTED_SHA256="$(awk -v name="$ARCHIVE_NAME" '$2 == name { print $1; exit }' "${WORK_DIR}/SHA256SUMS")"
if [[ ! "$EXPECTED_SHA256" =~ ^[[:xdigit:]]{64}$ ]]; then
    echo "ERROR: Ubuntu SHA256SUMS did not contain a valid checksum for $ARCHIVE_NAME." >&2
    exit 1
fi

printf '%s  %s\n' "$EXPECTED_SHA256" "$ARCHIVE_PATH" | sha256sum --check --status
echo "==> Ubuntu archive checksum verified: $EXPECTED_SHA256"

export HUB_ARCHIVE_PATH="$ARCHIVE_PATH"
export HUB_ARCHIVE_SHA256="$EXPECTED_SHA256"
export HUB_ARCHIVE_URL="$ARCHIVE_URL"
export HUB_MANIFEST_PATH="$MANIFEST_PATH"
export HUB_NELROOT_ARCH="$NELROOT_ARCH"

python3 - <<'PY'
import json
import os
from pathlib import Path

archive = Path(os.environ["HUB_ARCHIVE_PATH"])
manifest = {
    "schema_version": 1,
    "image": {
        "id": "atdev/ubuntu-24.04.5",
        "name": "Ubuntu Base",
        "version": "24.04.5",
        "distribution": "Ubuntu",
        "release": "24.04.5 Noble Numbat",
        "architecture": os.environ["HUB_NELROOT_ARCH"],
        "description": "Official minimal Ubuntu Base root filesystem for NELRoot. This is a base userspace, not a desktop or server ISO.",
        "maintainer": "AtDev",
        "license": "Ubuntu licensing terms",
        "tags": ["ubuntu", "noble", "24.04", "24.04.5", "base"],
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

echo "==> Publishing Ubuntu Base to the first-party NELRoot Image Hub..."
nelroot image hub-publish "$MANIFEST_PATH" "$ARCHIVE_PATH" --hub "$HUB_URL"

echo "==> Confirming the image is visible in the Hub catalog..."
nelroot image hub-remote-search ubuntu --hub "$HUB_URL" --architecture "$NELROOT_ARCH"

echo "==> Done. Pull it with:"
echo "nelroot image hub-pull atdev/ubuntu-24.04.5 --hub $HUB_URL --architecture $NELROOT_ARCH"
