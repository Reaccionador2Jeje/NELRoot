Name:           nelroot
Version:        0.1.1
Release:        1%{?dist}
Summary:        Independent Linux execution and virtualization engine
License:        MIT
URL:            https://github.com/Reaccionador2Jeje/NELRoot
Source0:        https://github.com/Reaccionador2Jeje/NELRoot/archive/refs/tags/v%{version}.tar.gz
BuildRequires:  cargo

%description
NELRoot is an independent systems project by AtDev. The current CLI
provides host diagnostics and Linux root filesystem image management.
It does not yet boot or execute a Linux guest.

%prep
%autosetup -n NELRoot-%{version}

%build
cargo build --release

%install
install -D -m 0755 target/release/nelroot %{buildroot}%{_bindir}/nelroot
install -D -m 0644 README.md %{buildroot}%{_docdir}/nelroot/README.md
install -D -m 0644 LICENSE %{buildroot}%{_docdir}/nelroot/LICENSE

%files
%{_bindir}/nelroot
%{_docdir}/nelroot/README.md
%{_docdir}/nelroot/LICENSE

%changelog
* Thu Nov 19 2026 AtDev - 0.1.1-1
- Initial source-build RPM specification.
