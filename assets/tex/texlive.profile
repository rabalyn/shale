# The install-tl profile for Shale's TeX Live (ADR-0019): only the infrastructure (the engines and
# tlmgr), no documentation and no sources. The packages come afterwards from packages.txt with
# `tlmgr install`. Used by tests/tex-env/Containerfile; the managed install
# (src/pdf/tex/install.rs, TEX-002) uses the same profile: the image's folder in the TEXDIR and TEXMF*
# lines becomes its own folder and the binary_ line the platform's.
selected_scheme scheme-infraonly
TEXDIR /opt/texlive
TEXMFLOCAL /opt/texlive/texmf-local
TEXMFSYSCONFIG /opt/texlive/texmf-config
TEXMFSYSVAR /opt/texlive/texmf-var
TEXMFCONFIG /opt/texlive/home/texmf-config
TEXMFHOME /opt/texlive/home/texmf
TEXMFVAR /opt/texlive/home/texmf-var
binary_x86_64-linux 1
# nothing is added to the system: no symlinks into /usr/local, no change of PATH files
instopt_adjustpath 0
# keep the repository the installer was started with (the current one at build time for the tests)
instopt_adjustrepo 0
instopt_letter 0
instopt_portable 0
instopt_write18_restricted 1
tlpdbopt_autobackup 0
tlpdbopt_backupdir tlpkg/backups
tlpdbopt_create_formats 1
tlpdbopt_desktop_integration 0
tlpdbopt_file_assocs 0
tlpdbopt_generate_updmap 0
tlpdbopt_install_docfiles 0
tlpdbopt_install_srcfiles 0
tlpdbopt_post_code 1
tlpdbopt_sys_bin /usr/local/bin
tlpdbopt_sys_info /usr/local/share/info
tlpdbopt_sys_man /usr/local/share/man
tlpdbopt_w32_multi_user 0
