#!/usr/bin/env bash
set -euo pipefail

REPO="Reaccionador2Jeje/NELRoot"
say() { printf "\033[1;32m[NELRoot]\033[0m %s\n" "$*"; }
fail() { printf "\033[1;31m[NELRoot] Error:\033[0m %s\n" "$*" >&2; exit 1; }

command -v tar >/dev/null 2>&1 || fail "tar is required to extract the release."
PYTHON=""
for candidate in python3 python; do
  if command -v "$candidate" >/dev/null 2>&1; then PYTHON="$candidate"; break; fi
done
[[ -n "$PYTHON" ]] || fail "Python 3 is required for HTTPS downloads. Install Python 3, then rerun this installer. No curl or wget is used."

OS="$(uname -s)"
ARCH="$(uname -m)"
if [[ -n "${TERMUX_VERSION:-}" || "${PREFIX:-}" == *com.termux* ]]; then
  PLATFORM="android"
  [[ "$ARCH" == "aarch64" || "$ARCH" == "arm64" ]] || fail "Termux release currently supports Android arm64 only."
elif [[ "$OS" == "Linux" ]]; then
  PLATFORM="linux"
elif [[ "$OS" == "Darwin" ]]; then
  PLATFORM="macos"
else
  fail "Unsupported operating system: $OS. Use the Windows PowerShell installer."
fi
case "$ARCH" in
  x86_64|amd64) CPU="x86_64" ;;
  aarch64|arm64) CPU="aarch64" ;;
  *) fail "Unsupported CPU architecture: $ARCH" ;;
esac

ASSET="nelroot-${PLATFORM}-${CPU}.tar.gz"
BASE_URL="https://github.com/${REPO}/releases/latest/download"
URL="${BASE_URL}/${ASSET}"
CHECKSUM_URL="${BASE_URL}/nelroot-${PLATFORM}-${CPU}.sha256"

say "NELRoot setup wizard"
say "Detected platform: ${PLATFORM}-${CPU}"
printf "Install for (u)ser only or (s)ystem-wide? [U/s]: "
read -r SCOPE
SCOPE="${SCOPE:-u}"

if [[ "$PLATFORM" == "android" ]]; then
  INSTALL_DIR="${NELROOT_INSTALL_DIR:-$PREFIX/bin}"
  NEED_SUDO=0
elif [[ "$SCOPE" =~ ^[Ss]$ ]]; then
  INSTALL_DIR="/usr/local/bin"
  NEED_SUDO=1
else
  INSTALL_DIR="${NELROOT_INSTALL_DIR:-$HOME/.local/bin}"
  NEED_SUDO=0
fi

printf "Installation directory [%s]: " "$INSTALL_DIR"
read -r CUSTOM_DIR
INSTALL_DIR="${CUSTOM_DIR:-$INSTALL_DIR}"

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT
say "Downloading release with Python HTTPS (no curl/wget)..."
"$PYTHON" - "$URL" "$CHECKSUM_URL" "$TMP_DIR/nelroot.tar.gz" <<'PY'
import hashlib
import sys
import urllib.request

archive_url, checksum_url, archive_path = sys.argv[1:4]
def download(url):
    request = urllib.request.Request(url, headers={"User-Agent": "NELRoot-Setup"})
    with urllib.request.urlopen(request, timeout=60) as response:
        return response.read()

try:
    archive = download(archive_url)
    checksum_text = download(checksum_url).decode("utf-8", errors="strict")
except Exception as exc:
    print(f"NELRoot download failed: {exc}", file=sys.stderr)
    raise SystemExit(1)

expected = checksum_text.strip().split()[0].lower() if checksum_text.strip() else ""
actual = hashlib.sha256(archive).hexdigest()
if len(expected) != 64 or actual != expected:
    print("NELRoot checksum verification failed; archive was not installed.", file=sys.stderr)
    raise SystemExit(1)

with open(archive_path, "wb") as output:
    output.write(archive)
print("Download complete; SHA-256 verified.")
PY
mkdir -p "$TMP_DIR/unpacked"
tar -xzf "$TMP_DIR/nelroot.tar.gz" -C "$TMP_DIR/unpacked"
[[ -f "$TMP_DIR/unpacked/nelroot" ]] || fail "The release archive does not contain the nelroot executable."

if [[ "$NEED_SUDO" -eq 1 ]]; then
  command -v sudo >/dev/null 2>&1 || fail "System-wide installation requires sudo, or choose user installation."
  sudo install -d "$INSTALL_DIR"
  sudo install -m 0755 "$TMP_DIR/unpacked/nelroot" "$INSTALL_DIR/nelroot"
else
  mkdir -p "$INSTALL_DIR"
  install -m 0755 "$TMP_DIR/unpacked/nelroot" "$INSTALL_DIR/nelroot"
fi

case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *)
    PROFILE="$HOME/.profile"
    case "${SHELL:-}" in
      */zsh) PROFILE="$HOME/.zprofile" ;;
      */bash) PROFILE="$HOME/.bashrc" ;;
    esac
    LINE="export PATH=\"$INSTALL_DIR:\$PATH\""
    touch "$PROFILE"
    grep -Fqx "$LINE" "$PROFILE" 2>/dev/null || printf '\n# NELRoot CLI\n%s\n' "$LINE" >> "$PROFILE"
    say "Added $INSTALL_DIR to PATH in $PROFILE"
    ;;
esac

say "Installation complete."
say "Open a new terminal or run: export PATH=\"$INSTALL_DIR:\$PATH\""
"$INSTALL_DIR/nelroot" --version
say "Try: nelroot --help"
