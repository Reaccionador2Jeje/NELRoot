#!/usr/bin/env bash
set -euo pipefail

VERSION="${1:?usage: build-rpm.sh VERSION TARGET_TRIPLE BINARY OUTPUT_DIR}"
TARGET="${2:?missing Rust target triple}"
BINARY="${3:?missing compiled binary path}"
OUTPUT_DIR="${4:?missing output directory}"

command -v rpmbuild >/dev/null 2>&1 || {
  echo "rpmbuild is required to build RPM packages." >&2
  exit 1
}
[[ -f "$BINARY" ]] || { echo "NELRoot binary not found: $BINARY" >&2; exit 1; }

case "$TARGET" in
  x86_64-unknown-linux-gnu) RPM_ARCH="x86_64" ;;
  aarch64-unknown-linux-gnu) RPM_ARCH="aarch64" ;;
  *) echo "Unsupported RPM target: $TARGET" >&2; exit 1 ;;
esac

TOP="$(mktemp -d)"
trap 'rm -rf "$TOP"' EXIT
mkdir -p "$TOP"/{BUILD,BUILDROOT,RPMS,SOURCES,SPECS,SRPMS}
install -m 0755 "$BINARY" "$TOP/SOURCES/nelroot"
install -m 0644 README.md "$TOP/SOURCES/README.md"
install -m 0644 LICENSE "$TOP/SOURCES/LICENSE"

cat > "$TOP/SPECS/nelroot.spec" <<'SPEC'
Name:           nelroot
Version:        %{_version}
Release:        1%{?dist}
Summary:        Independent Linux execution and virtualization engine
License:        MIT
BuildArch:      %{_target_cpu}
Source0:        nelroot
Source1:        README.md
Source2:        LICENSE

%description
NELRoot is an independent systems project by AtDev. The current CLI
manages host diagnostics and Linux root filesystem images. Guest
execution capabilities are developed separately and are not implied
by installation.

%prep

%build

%install
install -D -m 0755 %{SOURCE0} %{buildroot}%{_bindir}/nelroot
install -D -m 0644 %{SOURCE1} %{buildroot}%{_docdir}/nelroot/README.md
install -D -m 0644 %{SOURCE2} %{buildroot}%{_docdir}/nelroot/LICENSE

%files
%{_bindir}/nelroot
%{_docdir}/nelroot/README.md
%{_docdir}/nelroot/LICENSE

SPEC

rpmbuild -bb \
  --define "_topdir $TOP" \
  --define "_version $VERSION" \
  --define "_target_cpu $RPM_ARCH" \
  "$TOP/SPECS/nelroot.spec"

mkdir -p "$OUTPUT_DIR"
cp "$TOP/RPMS/$RPM_ARCH/nelroot-$VERSION-1.$RPM_ARCH.rpm" "$OUTPUT_DIR/nelroot-$VERSION-$RPM_ARCH.rpm"
