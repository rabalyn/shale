# ADR-0020: The managed TeX Live and the test image follow the current TeX Live

- **Status**: Accepted (owner decision, 2026-11-17)
- **Date**: 2026-11-17
- **Amends**: [ADR-0019](0019-latex-export-managed-texlive.md), decision 6 and the TL2025 figures
- **Specs**: TEX-002, TEX-020, TEX-022 ([`docs/spec/tex-export.md`](../spec/tex-export.md))
- **Issues**: #279

## Context

ADR-0019 had the test image built from the frozen TL2025 repository (`tlnet-final`) so that it was
reproducible, while the managed install used CTAN's current `tlnet`. TL2026 came out; there the
package list of ADR-0019 no longer worked unchanged (`l3backend` became part of `l3kernel`,
`pdfmanagement-testphase` became `pdfmanagement`), and `tlmgr` ends with an error for a package the
repository does not know. A test image on a different TeX Live than the one people get proves little.

## Decision

1. **The managed install uses the current TeX Live**: the default repository
   `https://mirror.ctan.org/systems/texlive/tlnet` (`tex.repository`, CFG-043), TL2026 today.
2. **The test image is built from the same repository**, not from a frozen one, so the tests check what
   users get. The image records the TeX Live year, the repository and the build date in
   `/etc/shale-tex-build`; `tex-env.sh status` and the demo test print it.
3. **`assets/tex/packages.txt` has the names of the current TeX Live.** A name the repository does not
   know is an error again; there is no skipping of missing packages.
4. The tuda-ci pin (commit `e933be9`, sha256) is unchanged.

## Consequences

- **The test image is not reproducible.** It is whatever TeX Live is current when it is built. A later
  year may rename or merge packages or change a template's behaviour; then the list or the templates
  change on purpose. `scripts/land.sh` uses the cached image; the CI job (tags and weekly) builds it with
  `tex-env.sh build --refresh` and runs the real install (`tex_install`), so upstream breakage shows
  within a week.
- **One mirror per install** (#320): `mirror.ctan.org` sends every request to some CTAN mirror, and the
  mirrors are not all equally current. When `install-tl` got a newer mirror and the following `tlmgr`
  an older one, `tlmgr` refused it ("older than the local installation"; Forgejo run #361). So the
  managed install and update (`install::resolve_mirror`) and the test image (`install-texlive.sh`)
  resolve the redirector once to one concrete mirror and use it for every step, and `tlmgr` tries a
  freshly resolved mirror when it still finds its mirror older (3 runs at most). The image records the
  mirror in `/etc/shale-tex-build`. The default repository stays the redirector, so people still get a
  mirror near them and the current TeX Live.
- **Measured 2026-11-17 against TL2026** (`scheme-infraonly` plus the list, no documentation, no
  sources): **326 MB installed**, and the install took **28 s on a fast mirror** (the packages are
  downloaded one by one; the speed depends on the mirror the redirector picks and varies, minutes on a
  slow one). The download is about 100 MB. The sizes shown before the download stay "about 100 MB" and
  "about 330 MB" (TEX-002).
