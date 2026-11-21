#!/usr/bin/env bash
set -Eeuo pipefail

# Publish every official rootfs variant currently supported by the NELRoot
# image publisher. Runs sequentially and skips image/architecture pairs already
# present in the Hub catalog.
#
# Usage:
#   export NELROOT_HUB_PUBLISH_TOKEN='...'
#   ./scripts/publish-all-images.sh
#
# Optional:
#   NELROOT_HUB_URL, ALPINE_VERSION

readonly HUB_URL="${NELROOT_HUB_URL:-https://nelroot-image-hub-production.up.railway.app}"
readonly ALPINE_VERSION="${ALPINE_VERSION:-3.24.4}"
readonly UBUNTU_ID="atdev/ubuntu-24.04.5"
readonly ALPINE_ID="atdev/alpine-${ALPINE_VERSION}"
readonly WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/nelroot-publish-all.XXXXXX")"

trap 'rm -rf "$WORK_DIR"' EXIT

if [[ "${1:-}" == "--list" ]]; then
    printf '%s\n' \
        "Ubuntu Base 24.04.5: amd64 (x86_64), arm64 (aarch64)" \
        "Alpine Mini Rootfs ${ALPINE_VERSION}: x86_64, aarch64, armv7, riscv64"
    exit 0
fi

if [[ $# -ne 0 ]]; then
    echo "Usage: $0 [--list]" >&2
    exit 2
fi

if [[ -z "${NELROOT_HUB_PUBLISH_TOKEN:-}" ]]; then
    echo "ERROR: export NELROOT_HUB_PUBLISH_TOKEN before publishing." >&2
    exit 1
fi
if (( ${#NELROOT_HUB_PUBLISH_TOKEN} < 32 )); then
    echo "ERROR: NELROOT_HUB_PUBLISH_TOKEN must contain at least 32 characters." >&2
    exit 1
fi

for command_name in curl python3 nelroot; do
    command -v "$command_name" >/dev/null 2>&1 || {
        echo "ERROR: required command not found: $command_name" >&2
        exit 1
    }
done

if [[ ! "$ALPINE_VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "ERROR: ALPINE_VERSION must look like 3.24.4." >&2
    exit 2
fi

published=0
skipped=0
failed=0

already_published() {
    local image_id="$1"
    local architecture="$2"
    local catalog_file="$WORK_DIR/catalog.json"

    curl --fail --location --silent --show-error --retry 3 \
        --proto '=https' --proto-redir '=https' \
        "$HUB_URL/v1/catalog" -o "$catalog_file"

    python3 - "$catalog_file" "$image_id" "$architecture" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as stream:
    catalog = json.load(stream)

found = any(
    image.get("id") == sys.argv[2]
    and image.get("architecture") == sys.argv[3]
    for image in catalog.get("images", [])
)
raise SystemExit(0 if found else 1)
PY
}

publish_one() {
    local family="$1"
    local image_id="$2"
    local nelroot_arch="$3"
    local publisher="$4"
    local publisher_arch="$5"

    echo
    echo "============================================================"
    echo "Image: $image_id ($nelroot_arch)"
    echo "============================================================"

    if already_published "$image_id" "$nelroot_arch"; then
        echo "SKIP: this image/architecture is already in the Hub catalog."
        ((skipped += 1))
        return 0
    fi

    if [[ "$family" == "ubuntu" ]]; then
        if bash "$publisher" "$publisher_arch"; then
            ((published += 1))
        else
            echo "ERROR: failed to publish $image_id ($nelroot_arch)." >&2
            ((failed += 1))
        fi
    else
        if ALPINE_VERSION="$ALPINE_VERSION" bash "$publisher" "$publisher_arch"; then
            ((published += 1))
        else
            echo "ERROR: failed to publish $image_id ($nelroot_arch)." >&2
            ((failed += 1))
        fi
    fi
}

echo "NELRoot Image Hub: $HUB_URL"
echo "Publishing all currently supported official rootfs variants."
echo "Existing catalog entries will be skipped; uploads run sequentially."

publish_one ubuntu "$UBUNTU_ID" x86_64 scripts/publish-ubuntu-base.sh amd64
publish_one ubuntu "$UBUNTU_ID" aarch64 scripts/publish-ubuntu-base.sh arm64

publish_one alpine "$ALPINE_ID" x86_64 scripts/publish-alpine-minirootfs.sh x86_64
publish_one alpine "$ALPINE_ID" aarch64 scripts/publish-alpine-minirootfs.sh aarch64
publish_one alpine "$ALPINE_ID" armv7 scripts/publish-alpine-minirootfs.sh armv7
publish_one alpine "$ALPINE_ID" riscv64 scripts/publish-alpine-minirootfs.sh riscv64

echo
echo "================ NELRoot Image Hub summary ================"
echo "Published: $published"
echo "Skipped:   $skipped"
echo "Failed:    $failed"
echo "==========================================================="

if (( failed > 0 )); then
    exit 1
fi

echo "All supported NELRoot images are now present in the Hub catalog."
echo "Search them with:"
echo "nelroot image hub-remote-search linux --hub $HUB_URL"
