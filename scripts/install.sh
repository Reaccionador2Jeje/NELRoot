#!/usr/bin/env bash
set -euo pipefail

REPO="Reaccionador2Jeje/NELRoot"
BIN_NAME="nelroot"

say() { printf "\033[1;32m[NELRoot]\033[0m %s\n" "$*"; }
fail() { printf "\033[1;31m[NELRoot] Error:\033[0m %s\n" "$*" >&2; exit 1; }

command -v curl >/dev/null 2>&1 || fail "curl is required."
command -v tar >/dev/null 2>&1 || fail "tar is required."

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
URL="https://github.com/${REPO}/releases/latest/download/${ASSET}"
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
say "Downloading ${ASSET}..."
curl -fL --retry 3 "$URL" -o "$TMP_DIR/nelroot.tar.gz" || fail "Download failed. Check whether a compatible GitHub Release exists."
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
    LINE="export PATH=\"$INSTALL_DIR:\$PATH\""
    touch "$PROFILE"
    grep -Fqx "$LINE" "$PROFILE" 2>/dev/null || printf '\n# NELRoot CLI\n%s\n' "$LINE" >> "$PROFILE"
    say "Added $INSTALL_DIR to PATH in $PROFILE"
    ;;
esac

say "Installation complete."
say "Open a new terminal or run: export PATH=\"$INSTALL_DIR:\$PATH\""
"$INSTALL_DIR/nelroot" --version
say "Try: nelroot help"
