#!/usr/bin/env bash
set -euo pipefail

VERSION="${1:?usage: build-deb.sh VERSION TARGET_TRIPLE BINARY OUTPUT_DIR}"
TARGET="${2:?missing Rust target triple}"
BINARY="${3:?missing compiled binary path}"
OUTPUT_DIR="${4:?missing output directory}"

command -v dpkg-deb >/dev/null 2>&1 || {
  echo "dpkg-deb is required to build Debian packages." >&2
  exit 1
}
[[ -x "$BINARY" || -f "$BINARY" ]] || {
  echo "NELRoot binary not found: $BINARY" >&2
  exit 1
}

case "$TARGET" in
  x86_64-unknown-linux-gnu) DEB_ARCH="amd64" ;;
  aarch64-unknown-linux-gnu) DEB_ARCH="arm64" ;;
  *) echo "Unsupported Debian target: $TARGET" >&2; exit 1 ;;
esac

ROOT="$(mktemp -d)"
trap 'rm -rf "$ROOT"' EXIT
mkdir -p "$ROOT/DEBIAN" "$ROOT/usr/bin" "$ROOT/usr/share/doc/nelroot"
install -m 0755 "$BINARY" "$ROOT/usr/bin/nelroot"
install -m 0644 README.md LICENSE "$ROOT/usr/share/doc/nelroot/"
sed -e "s/@VERSION@/$VERSION/g" -e "s/@ARCH@/$DEB_ARCH/g" \
  packaging/debian/control.in > "$ROOT/DEBIAN/control"
mkdir -p "$OUTPUT_DIR"
dpkg-deb --root-owner-group --build "$ROOT" "$OUTPUT_DIR/nelroot_${VERSION}_${DEB_ARCH}.deb"
