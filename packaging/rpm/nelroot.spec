Name:           nelroot
Version:        %{?nelroot_version}%{!?nelroot_version:0.1.0}
Release:        1%{?dist}
Summary:        Independent Linux execution and virtualization engine
License:        MIT
URL:            https://github.com/Reaccionador2Jeje/NELRoot
BuildArch:      %{_arch}

%description
NELRoot is an independent systems project by AtDev. The current CLI
provides host diagnostics and Linux root filesystem image management.
It does not yet boot or execute a Linux guest.

%install
install -D -m 0755 %{_sourcedir}/nelroot %{buildroot}%{_bindir}/nelroot
install -D -m 0644 %{_sourcedir}/README.md %{buildroot}%{_docdir}/nelroot/README.md
install -D -m 0644 %{_sourcedir}/LICENSE %{buildroot}%{_docdir}/nelroot/LICENSE

%files
%{_bindir}/nelroot
%{_docdir}/nelroot/README.md
%{_docdir}/nelroot/LICENSE

%changelog
* Thu Nov 19 2026 AtDev <packages@atdev.dev> - 0.1.0-1
- Initial packaging specification.
