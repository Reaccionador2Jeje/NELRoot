#!/usr/bin/env bash
set -Eeuo pipefail
# Official Fedora Container Base publisher. Optional: FEDORA_VERSION, FEDORA_COMPOSE.
HUB_URL="${NELROOT_HUB_URL:-https://nelroot-image-hub-production.up.railway.app}"
VERSION="${FEDORA_VERSION:-45}"
COMPOSE="${FEDORA_COMPOSE:-1.4}"
if [[ -z "${NELROOT_HUB_PUBLISH_TOKEN:-}" || ${#NELROOT_HUB_PUBLISH_TOKEN} -lt 32 ]]; then echo "Set NELROOT_HUB_PUBLISH_TOKEN (32+ chars)." >&2; exit 1; fi
[[ "$VERSION" =~ ^[0-9]+$ && "$COMPOSE" =~ ^[0-9]+([.][0-9]+)*$ ]] || { echo "Invalid Fedora version/compose" >&2; exit 2; }
case "${1:-x86_64}" in
 x86_64|amd64) ARCH=x86_64; NARCH=x86_64; TREE=linux/releases ;;
 aarch64|arm64) ARCH=aarch64; NARCH=aarch64; TREE=linux/releases ;;
 ppc64le) ARCH=ppc64le; NARCH=ppc64le; TREE=secondary/releases ;;
 s390x) ARCH=s390x; NARCH=s390x; TREE=secondary/releases ;;
 *) echo "Usage: $0 [x86_64|aarch64|ppc64le|s390x]" >&2; exit 2 ;;
esac
BASE="https://dl.fedoraproject.org/pub/fedora/$TREE/$VERSION/Container/$ARCH/images"
CHECKSUM="Fedora-Container-$VERSION-$COMPOSE-$ARCH-CHECKSUM"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/nelroot-fedora.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
for cmd in curl sha256sum awk tar xz python3 nelroot; do command -v "$cmd" >/dev/null || { echo "Missing command: $cmd" >&2; exit 1; }; done
mkdir -p "$WORK/rootfs"
curl -fsSL --retry 3 "$BASE/$CHECKSUM" -o "$WORK/$CHECKSUM"
NAME="$(awk '$2 ~ /^\*?Fedora-Container-Base-.*\.tar\.xz$/ {sub(/^\*/, "", $2); print $2; exit}' "$WORK/$CHECKSUM")"
[[ -n "$NAME" ]] || { echo "Fedora CHECKSUM has no Container Base tar.xz entry" >&2; exit 1; }
URL="$BASE/$NAME"
curl -fL --retry 3 "$URL" -o "$WORK/$NAME"
HASH="$(awk -v n="$NAME" '$2 == n || $2 == "*" n {print $1; exit}' "$WORK/$CHECKSUM")"
[[ "$HASH" =~ ^[[:xdigit:]]{64}$ ]] || { echo "No valid Fedora SHA256 for $NAME" >&2; exit 1; }
printf '%s  %s\n' "$HASH" "$WORK/$NAME" | sha256sum -c --status
tar -xJf "$WORK/$NAME" -C "$WORK/rootfs"
tar --numeric-owner -czf "$WORK/rootfs.tar.gz" -C "$WORK/rootfs" .
ROOT_HASH="$(sha256sum "$WORK/rootfs.tar.gz" | awk '{print $1}')"
export HUB_ARCHIVE_PATH="$WORK/rootfs.tar.gz" HUB_ARCHIVE_SHA256="$ROOT_HASH" HUB_ARCHIVE_URL="$URL" HUB_MANIFEST_PATH="$WORK/manifest.json"
export HUB_VERSION="$VERSION" HUB_COMPOSE="$COMPOSE" HUB_NARCH="$NARCH"
python3 - <<'PY'
import json, os
from pathlib import Path
p=Path(os.environ["HUB_ARCHIVE_PATH"]); v=os.environ["HUB_VERSION"]; c=os.environ["HUB_COMPOSE"]
m={"schema_version":1,"image":{"id":f"atdev/fedora-{v}","name":"Fedora Container Base","version":f"{v}.{c}","distribution":"Fedora Linux","release":v,"architecture":os.environ["HUB_NARCH"],"description":"Official Fedora Container Base root filesystem normalized to tar.gz by NELRoot.","maintainer":"AtDev","license":"Mixed","tags":["fedora","linux","container-base",v,"minimal"],"rootfs":{"format":"tar.gz","size_bytes":p.stat().st_size,"sha256":os.environ["HUB_ARCHIVE_SHA256"],"download_url":os.environ["HUB_ARCHIVE_URL"]}}}
Path(os.environ["HUB_MANIFEST_PATH"]).write_text(json.dumps(m,indent=2)+"\n",encoding="utf-8")
PY
nelroot image hub-manifest-validate "$WORK/manifest.json"
nelroot image hub-publish "$WORK/manifest.json" "$WORK/rootfs.tar.gz" --hub "$HUB_URL"
nelroot image hub-remote-search fedora --hub "$HUB_URL" --architecture "$NARCH"
echo "Pull: nelroot image hub-pull atdev/fedora-$VERSION --hub $HUB_URL --architecture $NARCH"
