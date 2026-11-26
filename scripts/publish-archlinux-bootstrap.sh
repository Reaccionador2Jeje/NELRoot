#!/usr/bin/env bash
set -Eeuo pipefail
# Official Arch Linux bootstrap publisher. Requires curl, sha256sum, tar, zstd, python3, nelroot.
HUB_URL="${NELROOT_HUB_URL:-https://nelroot-image-hub-production.up.railway.app}"
BASE_URL="${ARCH_RELEASE_URL:-https://geo.mirror.pkgbuild.com/iso/latest}"
NAME="archlinux-bootstrap-x86_64.tar.zst"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/nelroot-arch.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
for cmd in curl sha256sum tar zstd python3 nelroot; do command -v "$cmd" >/dev/null || { echo "Missing command: $cmd" >&2; exit 1; }; done
if [[ -z "${NELROOT_HUB_PUBLISH_TOKEN:-}" || ${#NELROOT_HUB_PUBLISH_TOKEN} -lt 32 ]]; then echo "Set NELROOT_HUB_PUBLISH_TOKEN (32+ chars)." >&2; exit 1; fi
mkdir -p "$WORK/extract" "$WORK/rootfs"
curl -fsSL --retry 3 "$BASE_URL/sha256sums.txt" -o "$WORK/sha256sums.txt"
curl -fL --retry 3 "$BASE_URL/$NAME" -o "$WORK/$NAME"
HASH="$(awk -v n="$NAME" '$2 == n || $2 == "*" n {print $1; exit}' "$WORK/sha256sums.txt")"
[[ "$HASH" =~ ^[[:xdigit:]]{64}$ ]] || { echo "No valid Arch checksum for $NAME" >&2; exit 1; }
printf '%s  %s\n' "$HASH" "$WORK/$NAME" | sha256sum -c --status
tar --zstd --no-same-owner --no-same-permissions --mode='u+rwX' -xf "$WORK/$NAME" -C "$WORK/extract"
ROOT="$WORK/extract/root.x86_64"
if [[ ! -d "$ROOT" ]]; then ROOT="$(find "$WORK/extract" -mindepth 2 -maxdepth 3 -type d -name root.x86_64 -print -quit)"; fi
[[ -n "$ROOT" && -d "$ROOT" ]] || { echo "Arch bootstrap is missing root.x86_64" >&2; exit 1; }
tar --numeric-owner --owner=0 --group=0 -czf "$WORK/rootfs.tar.gz" -C "$ROOT" .
ROOT_HASH="$(sha256sum "$WORK/rootfs.tar.gz" | awk '{print $1}')"
export HUB_ARCHIVE_PATH="$WORK/rootfs.tar.gz" HUB_ARCHIVE_SHA256="$ROOT_HASH" HUB_ARCHIVE_URL="$BASE_URL/$NAME" HUB_MANIFEST_PATH="$WORK/manifest.json"
python3 - <<'PY'
import json, os
from pathlib import Path
p = Path(os.environ["HUB_ARCHIVE_PATH"])
m = {"schema_version":1,"image":{"id":"atdev/archlinux-bootstrap","name":"Arch Linux Bootstrap","version":"rolling","distribution":"Arch Linux","release":"Rolling","architecture":"x86_64","description":"Official Arch Linux bootstrap root filesystem normalized to tar.gz by NELRoot.","maintainer":"AtDev","license":"Mixed","tags":["arch","archlinux","rolling","bootstrap","minimal"],"rootfs":{"format":"tar.gz","size_bytes":p.stat().st_size,"sha256":os.environ["HUB_ARCHIVE_SHA256"],"download_url":os.environ["HUB_ARCHIVE_URL"]}}}
Path(os.environ["HUB_MANIFEST_PATH"]).write_text(json.dumps(m,indent=2)+"\n",encoding="utf-8")
PY
nelroot image hub-manifest-validate "$WORK/manifest.json"
nelroot image hub-publish "$WORK/manifest.json" "$WORK/rootfs.tar.gz" --hub "$HUB_URL"
nelroot image hub-remote-search arch --hub "$HUB_URL" --architecture x86_64
echo "Pull: nelroot image hub-pull atdev/archlinux-bootstrap --hub $HUB_URL --architecture x86_64"
