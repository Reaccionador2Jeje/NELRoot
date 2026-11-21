#!/usr/bin/env bash
set -Eeuo pipefail

# Publish every official rootfs variant currently supported by the NELRoot
# image publisher. Runs sequentially and skips image/architecture pairs already
# present in the Hub catalog.
#
# Usage:
#   export NELROOT_HUB_PUBLISH_TOKEN='...'
#   bash scripts/publish-all-images.sh
#
# Optional:
#   NELROOT_HUB_URL, ALPINE_VERSION

readonly HUB_URL="${NELROOT_HUB_URL:-https://nelroot-image-hub-production.up.railway.app}"
readonly ALPINE_VERSION="${ALPINE_VERSION:-3.24.4}"
readonly UBUNTU_ID="atdev/ubuntu-24.04.5"
readonly ALPINE_ID="atdev/alpine-${ALPINE_VERSION}"
readonly VOID_RELEASE_DATE="${VOID_RELEASE_DATE:-20250202}"
readonly FEDORA_VERSION="${FEDORA_VERSION:-45}"
readonly FEDORA_COMPOSE="${FEDORA_COMPOSE:-1.4}"
readonly WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/nelroot-publish-all.XXXXXX")"

trap 'rm -rf "$WORK_DIR"' EXIT

if [[ "${1:-}" == "--list" ]]; then
    printf '%s\n' \
        "Ubuntu Base 24.04.5: amd64 (x86_64), arm64 (aarch64), armhf (arm), ppc64el (ppc64le), riscv64, s390x" \
        "Alpine Mini Rootfs ${ALPINE_VERSION}: x86_64, aarch64, armv7, riscv64" \
        "Arch Linux Bootstrap (rolling): x86_64" \
        "Void Linux ROOTFS ${VOID_RELEASE_DATE} (glibc): x86_64, aarch64, armv7" \
        "Fedora Container Base ${FEDORA_VERSION}.${FEDORA_COMPOSE}: x86_64, aarch64, ppc64le, s390x"
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
        "$HUB_URL/v1/catalog" -o "$catalog_file" || return 2

    local result
    result="$(python3 - "$catalog_file" "$image_id" "$architecture" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as stream:
    catalog = json.load(stream)

found = any(
    image.get("id") == sys.argv[2]
    and image.get("architecture") == sys.argv[3]
    for image in catalog.get("images", [])
)
print("yes" if found else "no")
PY
)" || return 2

    [[ "$result" == "yes" ]] && return 0
    return 1
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

    local catalog_status=0
    if already_published "$image_id" "$nelroot_arch"; then
        echo "SKIP: this image/architecture is already in the Hub catalog."
        ((skipped += 1))
        return 0
    else
        catalog_status=$?
    fi

    if (( catalog_status != 1 )); then
        echo "ERROR: could not safely inspect the Hub catalog; refusing to publish." >&2
        ((failed += 1))
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
        local publish_status=0
        case "$family" in
            alpine)
                ALPINE_VERSION="$ALPINE_VERSION" bash "$publisher" "$publisher_arch" || publish_status=$?
                ;;
            arch)
                bash "$publisher" || publish_status=$?
                ;;
            void)
                VOID_RELEASE_DATE="$VOID_RELEASE_DATE" VOID_LIBC=glibc bash "$publisher" "$publisher_arch" || publish_status=$?
                ;;
            fedora)
                FEDORA_VERSION="$FEDORA_VERSION" FEDORA_COMPOSE="$FEDORA_COMPOSE" bash "$publisher" "$publisher_arch" || publish_status=$?
                ;;
            *)
                echo "ERROR: unknown publisher family: $family" >&2
                publish_status=2
                ;;
        esac
        if (( publish_status == 0 )); then
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
publish_one ubuntu "$UBUNTU_ID" arm scripts/publish-ubuntu-base.sh armhf
publish_one ubuntu "$UBUNTU_ID" ppc64le scripts/publish-ubuntu-base.sh ppc64el
publish_one ubuntu "$UBUNTU_ID" riscv64 scripts/publish-ubuntu-base.sh riscv64
publish_one ubuntu "$UBUNTU_ID" s390x scripts/publish-ubuntu-base.sh s390x

publish_one alpine "$ALPINE_ID" x86_64 scripts/publish-alpine-minirootfs.sh x86_64
publish_one alpine "$ALPINE_ID" aarch64 scripts/publish-alpine-minirootfs.sh aarch64
publish_one alpine "$ALPINE_ID" armv7 scripts/publish-alpine-minirootfs.sh armv7
publish_one alpine "$ALPINE_ID" riscv64 scripts/publish-alpine-minirootfs.sh riscv64

publish_one arch atdev/archlinux-bootstrap x86_64 scripts/publish-archlinux-bootstrap.sh ""

publish_one void "atdev/void-${VOID_RELEASE_DATE}-glibc" x86_64 scripts/publish-void-rootfs.sh x86_64
publish_one void "atdev/void-${VOID_RELEASE_DATE}-glibc" aarch64 scripts/publish-void-rootfs.sh aarch64
publish_one void "atdev/void-${VOID_RELEASE_DATE}-glibc" armv7 scripts/publish-void-rootfs.sh armv7l

publish_one fedora "atdev/fedora-${FEDORA_VERSION}" x86_64 scripts/publish-fedora-container.sh x86_64
publish_one fedora "atdev/fedora-${FEDORA_VERSION}" aarch64 scripts/publish-fedora-container.sh aarch64
publish_one fedora "atdev/fedora-${FEDORA_VERSION}" ppc64le scripts/publish-fedora-container.sh ppc64le
publish_one fedora "atdev/fedora-${FEDORA_VERSION}" s390x scripts/publish-fedora-container.sh s390x

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
