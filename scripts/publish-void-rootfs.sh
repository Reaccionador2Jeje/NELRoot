#!/usr/bin/env bash
set -Eeuo pipefail
# Official Void Linux ROOTFS publisher. Optional: VOID_RELEASE_DATE, VOID_LIBC.
HUB_URL="${NELROOT_HUB_URL:-https://nelroot-image-hub-production.up.railway.app}"
DATE="${VOID_RELEASE_DATE:-20250202}"
LIBC="${VOID_LIBC:-glibc}"
BASE="https://repo-default.voidlinux.org/live/current"
if [[ -z "${NELROOT_HUB_PUBLISH_TOKEN:-}" || ${#NELROOT_HUB_PUBLISH_TOKEN} -lt 32 ]]; then echo "Set NELROOT_HUB_PUBLISH_TOKEN (32+ chars)." >&2; exit 1; fi
[[ "$DATE" =~ ^[0-9]{8}$ ]] || { echo "VOID_RELEASE_DATE must be YYYYMMDD" >&2; exit 2; }
[[ "$LIBC" == glibc || "$LIBC" == musl ]] || { echo "VOID_LIBC must be glibc or musl" >&2; exit 2; }
case "${1:-x86_64}" in
 x86_64) ARCH=x86_64; NARCH=x86_64 ;;
 aarch64) ARCH=aarch64; NARCH=aarch64 ;;
 armv7l) ARCH=armv7l; NARCH=armv7 ;;
 *) echo "Usage: $0 [x86_64|aarch64|armv7l]" >&2; exit 2 ;;
esac
SUFFIX=""; [[ "$LIBC" == musl ]] && SUFFIX="-musl"
NAME="void-$ARCH$SUFFIX-ROOTFS-$DATE.tar.xz"
URL="$BASE/$NAME"
IMAGE_ID="atdev/void-$DATE-$LIBC"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/nelroot-void.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
for cmd in curl sha256sum awk tar xz python3 nelroot; do command -v "$cmd" >/dev/null || { echo "Missing command: $cmd" >&2; exit 1; }; done
mkdir -p "$WORK/rootfs"
curl -fsSL --retry 3 "$BASE/sha256sum.txt" -o "$WORK/sha256sum.txt"
curl -fL --retry 3 "$URL" -o "$WORK/$NAME"
HASH="$(awk -v n="$NAME" '$2 == n || $2 == "*" n {print $1; exit}' "$WORK/sha256sum.txt")"
[[ "$HASH" =~ ^[[:xdigit:]]{64}$ ]] || { echo "No valid Void checksum for $NAME" >&2; exit 1; }
printf '%s  %s\n' "$HASH" "$WORK/$NAME" | sha256sum -c --status
tar -xJf "$WORK/$NAME" -C "$WORK/rootfs"
tar --numeric-owner -czf "$WORK/rootfs.tar.gz" -C "$WORK/rootfs" .
ROOT_HASH="$(sha256sum "$WORK/rootfs.tar.gz" | awk '{print $1}')"
export HUB_ARCHIVE_PATH="$WORK/rootfs.tar.gz" HUB_ARCHIVE_SHA256="$ROOT_HASH" HUB_ARCHIVE_URL="$URL" HUB_MANIFEST_PATH="$WORK/manifest.json"
export HUB_IMAGE_ID="$IMAGE_ID" HUB_NARCH="$NARCH" HUB_LIBC="$LIBC" HUB_DATE="$DATE"
python3 - <<'PY'
import json, os
from pathlib import Path
p=Path(os.environ["HUB_ARCHIVE_PATH"]); libc=os.environ["HUB_LIBC"]; date=os.environ["HUB_DATE"]
m={"schema_version":1,"image":{"id":os.environ["HUB_IMAGE_ID"],"name":f"Void Linux ROOTFS ({libc})","version":date,"distribution":"Void Linux","release":date,"architecture":os.environ["HUB_NARCH"],"description":f"Official Void Linux {libc} ROOTFS normalized to tar.gz by NELRoot.","maintainer":"AtDev","license":"Mixed","tags":["void","voidlinux","rootfs",libc,date],"rootfs":{"format":"tar.gz","size_bytes":p.stat().st_size,"sha256":os.environ["HUB_ARCHIVE_SHA256"],"download_url":os.environ["HUB_ARCHIVE_URL"]}}}
Path(os.environ["HUB_MANIFEST_PATH"]).write_text(json.dumps(m,indent=2)+"\n",encoding="utf-8")
PY
nelroot image hub-manifest-validate "$WORK/manifest.json"
nelroot image hub-publish "$WORK/manifest.json" "$WORK/rootfs.tar.gz" --hub "$HUB_URL"
nelroot image hub-remote-search void --hub "$HUB_URL" --architecture "$NARCH"
echo "Pull: nelroot image hub-pull $IMAGE_ID --hub $HUB_URL --architecture $NARCH"
