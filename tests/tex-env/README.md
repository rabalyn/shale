# TeX test environment

A container image with a real LuaLaTeX and `biber` (TeX Live 2025), and the TU Darmstadt
templates [tuda-ci](https://github.com/tudace/tuda_latex_templates) 4.06, to test Shale's LaTeX
export against a real TeX (TEX-020, TEX-021, [ADR-0019](../../docs/decisions/0019-latex-export-managed-texlive.md)).
Nothing runs in the background: the image is built once, each compile is a short-lived container.

Needs `podman` (or Docker: `SHALE_TEX_ENV_ENGINE=docker`), `curl` and `sha256sum` (or `shasum`).
The base image is pinned (`docker.io/library/debian:bookworm-20260112-slim`).

## Use

```sh
tests/tex-env/tex-env.sh build      # the image, the tuda-ci sources, the generated classes
tests/tex-env/tex-env.sh status
tests/tex-env/tex-env.sh compile <dir> <file.tex>   # latexmk (LuaLaTeX, biber) in <dir>, mounted as /work
tests/tex-env/tex-env.sh exec <dir> -- <cmd>…   # any command in the image, <dir> (absolute) mounted at the same path
tests/tex-env/tex-env.sh reset      # remove the image and the cache
cargo test -- --ignored tex_env     # the 25 demos, and Shale's own compile and detection code (src/pdf/tex/)
cargo test -- --ignored tex_install # not this image: installs TeX Live for real from the frozen repository (network, minutes)
```

`exec` is how `src/pdf/tex/compile_tests.rs` runs Shale's compile code against the real LuaLaTeX: the test's `Engine`
has a command prefix `sh tex-env.sh exec <work folder> --`, so `lualatex`, `biber` and `kpsewhich` run in the
container with the work folder mounted at the same path; `TEXINPUTS`, `BIBINPUTS`, `openout_any` and
`shell_escape` are passed on when set.

`compile` sets `TEXINPUTS=/work/tex//:` and `BIBINPUTS=/work:`; `SHALE_TEX_PRE` is TeX code put in
front of the file (the tests use graphicx' `demo` option and the version macros, like the
upstream `latexmkrc`).

## What is in it

- **TeX Live from a frozen repository**: `install-tl` with `assets/tex/texlive.profile`
  (`scheme-infraonly`) from the TL2025 final repository at `ftp.math.utah.edu`, so the image is the
  same whenever it is built; then the packages of `assets/tex/packages.txt` (one list, shared with
  the managed install of the future). The packages are not minimised beyond "the 25 demos compile".
- **tuda-ci at commit `e933be93…`** (v4.06; there is no tag): GitHub's archive, checked by sha256, in
  `tests/tex-env/cache/` (git-ignored). `build` runs docstrip (`latex tuda-ci.ins`) in the
  image to make the `.cls`/`.sty` files from the `.dtx`.
- **A stand-in logo**: the TU logo cannot be redistributed, and the classes look for
  `tuda_logo.pdf` while the theme loads, before the `demo` option takes effect. `build` makes a grey
  bar of that name in `cache/tuda-ci/tex/`.

## The first build

It downloads about 94 MB of TeX Live packages and takes a few minutes. The image is about 470 MB.
After an edit of `packages.txt`, `build` runs the package step again.
