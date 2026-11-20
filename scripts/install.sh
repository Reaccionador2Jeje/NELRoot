#!/usr/bin/env bash
set -euo pipefail

REPO="Reaccionador2Jeje/NELRoot"
say() { printf "\033[1;32m[NELRoot]\033[0m %s\n" "$*"; }
fail() { printf "\033[1;31m[NELRoot] Error:\033[0m %s\n" "$*" >&2; exit 1; }

command -v tar >/dev/null 2>&1 || fail "tar is required to extract the release."
command -v wget >/dev/null 2>&1 || fail "wget is required. Install it with 'sudo apt install wget' on Ubuntu/Debian or 'pkg install wget' on Termux."

if command -v sha256sum >/dev/null 2>&1; then
  hash_file() { sha256sum "$1" | awk '{print $1}'; }
elif command -v shasum >/dev/null 2>&1; then
  hash_file() { shasum -a 256 "$1" | awk '{print $1}'; }
else
  fail "A SHA-256 utility is required (sha256sum or shasum)."
fi

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
say "Downloader: wget"
printf "Install for (u)ser only or (s)ystem-wide? [U/s]: "
read -r SCOPE
SCOPE="${SCOPE:-u}"

if [[ "$PLATFORM" == "android" ]]; then
  INSTALL_DIR="${NELROOT_INSTALL_DIR:-$PREFIX/bin}"
  NEED_SUDO=0
elif [[ "$SCOPE" =~ ^[Ss]$ ]]; then
  INSTALL_DIR="/usr/local/bin"
  if [[ "${EUID:-$(id -u)}" -eq 0 ]]; then NEED_SUDO=0; else NEED_SUDO=1; fi
else
  INSTALL_DIR="${NELROOT_INSTALL_DIR:-$HOME/.local/bin}"
  NEED_SUDO=0
fi

printf "Installation directory [%s]: " "$INSTALL_DIR"
read -r CUSTOM_DIR
INSTALL_DIR="${CUSTOM_DIR:-$INSTALL_DIR}"

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT
RELEASE_READY=0

say "Checking the latest GitHub Release..."
PROBE_OUTPUT="$(wget --server-response --spider --timeout=30 "$URL" 2>&1 || true)"
if grep -Eq 'HTTP/[0-9.]+ 404' <<< "$PROBE_OUTPUT"; then
  say "No published binary asset found. Preparing a source build."
else
  say "Downloading release archive and checksum with wget..."
  wget --show-progress --timeout=120 -O "$TMP_DIR/nelroot.tar.gz" "$URL" || fail "Release archive download failed."
  wget -q --timeout=60 -O "$TMP_DIR/nelroot.sha256" "$CHECKSUM_URL" || fail "Release checksum download failed."
  EXPECTED="$(awk 'NR == 1 { print tolower($1); exit }' "$TMP_DIR/nelroot.sha256")"
  ACTUAL="$(hash_file "$TMP_DIR/nelroot.tar.gz")"
  [[ "$EXPECTED" =~ ^[0-9a-f]{64}$ ]] || fail "The release checksum file is malformed."
  [[ "$ACTUAL" == "$EXPECTED" ]] || fail "SHA-256 verification failed. The downloaded archive was not installed."
  RELEASE_READY=1
  say "Release downloaded and SHA-256 verified."
fi

if [[ "$RELEASE_READY" -eq 0 ]]; then
  command -v cargo >/dev/null 2>&1 || fail "No published release exists and Cargo is not installed. Install stable Rust/Cargo, then rerun the installer."
  say "Downloading the latest source snapshot with wget..."
  wget -q --timeout=60 -O "$TMP_DIR/commit.json" "https://api.github.com/repos/$REPO/commits/main" || fail "Could not resolve the latest source commit."
  COMMIT="$(sed -nE 's/.*"sha"[[:space:]]*:[[:space:]]*"([0-9a-f]{40})".*/\1/p' "$TMP_DIR/commit.json" | head -n 1)"
  [[ "$COMMIT" =~ ^[0-9a-f]{40}$ ]] || fail "GitHub returned an invalid source commit identifier."
  wget --show-progress --timeout=120 -O "$TMP_DIR/source.tar.gz" "https://codeload.github.com/$REPO/tar.gz/$COMMIT" || fail "Source archive download failed."
  mkdir -p "$TMP_DIR/source"
  tar -xzf "$TMP_DIR/source.tar.gz" -C "$TMP_DIR/source"
  SOURCE_DIR="$(find "$TMP_DIR/source" -mindepth 1 -maxdepth 1 -type d | head -n 1)"
  [[ -n "$SOURCE_DIR" && -f "$SOURCE_DIR/Cargo.toml" ]] || fail "Downloaded source archive is incomplete."
  say "Compiling NELRoot from source. This can take several minutes..."
  (cd "$SOURCE_DIR" && cargo build --release)
  mkdir -p "$TMP_DIR/unpacked"
  install -m 0755 "$SOURCE_DIR/target/release/nelroot" "$TMP_DIR/unpacked/nelroot"
else
  mkdir -p "$TMP_DIR/unpacked"
  tar -xzf "$TMP_DIR/nelroot.tar.gz" -C "$TMP_DIR/unpacked"
fi

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
