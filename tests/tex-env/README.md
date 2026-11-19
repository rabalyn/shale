# TeX test environment

A container image with a real LuaLaTeX and `biber` (the current TeX Live, 2026 at the time of writing), and the TU Darmstadt
templates [tuda-ci](https://github.com/tudace/tuda_latex_templates) 4.06, to test Shale's LaTeX
export against a real TeX (TEX-020, TEX-021, [ADR-0019](../../docs/decisions/0019-latex-export-managed-texlive.md)).
Nothing runs in the background: the image is built once, each compile is a short-lived container.

Needs `podman` (or Docker: `SHALE_TEX_ENV_ENGINE=docker`), `curl` and `sha256sum` (or `shasum`).
The image is called `shale-tex-env` for every checkout of Shale on the machine, so a `build` in one checkout replaces the image
under a test that runs in another: `SHALE_TEX_ENV_IMAGE=<name>` gives a checkout its own image.
The base image is pinned (`docker.io/library/debian:bookworm-20260112-slim`).

## Use

```sh
tests/tex-env/tex-env.sh build      # (--refresh: without the cache) the image, the tuda-ci sources, the generated classes
tests/tex-env/tex-env.sh status
tests/tex-env/tex-env.sh compile <dir> <file.tex>   # latexmk (LuaLaTeX, biber) in <dir>, mounted as /work
tests/tex-env/tex-env.sh exec <dir> -- <cmd>…   # any command in the image, <dir> (absolute) mounted at the same path
tests/tex-env/tex-env.sh reset      # remove the image and the cache
cargo test -- --ignored tex_env     # the 25 demos, Shale's own compile and detection code, and the template sets (import of the cached archive, all 11 kinds) (src/pdf/tex/)
cargo test -- --ignored tex_import  # downloads tuda-ci from GitHub and CTAN (a few MB) and imports it with docstrip in this image
cargo test -- --ignored tex_install # not this image: installs TeX Live for real from the default repository (network, minutes)
```

The `tex_env` tests share nothing (own work folders, a fresh container per compile), so they can run in parallel:
`.forgejo/scripts/env-tests.sh run` (and so `scripts/land.sh`) uses four test threads, `SHALE_ENV_TESTS_TEX_THREADS`
sets the number (the demos test starts four compiles itself; #370). `tex_import` and `tex_install` run with one thread.

**Failed compiles keep their log** (#327): `SHALE_TEX_FAILED_LOGS=<folder>` makes every failed compile of the tests (Shale's
own `compile` and the demos of `tex_env`) copy its full `.log` into that folder as `<pid>-<n>-<name>.log` before its work
folder is removed, so a failure that does not come again can still be read. `.forgejo/scripts/env-tests.sh run` (and so
`scripts/land.sh`) sets it to `target/tex-failed-logs` (emptied at the start) and lists the kept logs at the end.
When `tex_env` fails with a font that could not be loaded, `env-tests.sh` says it looks like a TeX environment problem,
not a code error, prints `tex-env.sh status`, rebuilds the image with `build --refresh` (a freshly resolved mirror) and
runs `tex_env` once more; the second run decides, and any other failure is not retried
(`scripts/test-env-tests.sh` tests that).

`exec` is how `src/pdf/tex/compile_tests.rs` runs Shale's compile code against the real LuaLaTeX: the test's `Engine`
has a command prefix `sh tex-env.sh exec <work folder> --`, so `lualatex`, `biber` and `kpsewhich` run in the
container with the work folder mounted at the same path (the program runs in the folder `exec` was started in when that is inside the mounted one, which docstrip in the template import tests needs); `TEXINPUTS`, `BIBINPUTS`, `openout_any` and
`shell_escape` are passed on when set.

`compile` sets `TEXINPUTS=/work/tex//:` and `BIBINPUTS=/work:`; `SHALE_TEX_PRE` is TeX code put in
front of the file (the tests use graphicx' `demo` option and the version macros, like the
upstream `latexmkrc`).

## What is in it

- **The current TeX Live** (ADR-0020): `install-tl` with `assets/tex/texlive.profile`
  (`scheme-infraonly`) from the default repository `https://mirror.ctan.org/systems/texlive/tlnet`, the one
  the managed install uses, then the packages of `assets/tex/packages.txt` (one list, shared with the
  managed install). `install-texlive.sh` does it in one step: the redirector is resolved once to one
  mirror, which `install-tl` and `tlmgr` both use (two mirrors of different age made `tlmgr` refuse,
  #320), and `tlmgr` tries a fresh mirror when its mirror is older than the installation (three runs at
  most). So the image is **not reproducible**: it is whatever TeX Live is current when it is
  built. It records which one in `/etc/shale-tex-build` (year, repository, mirror, date), which `status` and the
  demo test print. `build` reuses a cached image; `build --refresh` rebuilds it without the cache, which the
  weekly and tag CI runs do, so a change upstream shows within a week.
- **tuda-ci at commit `e933be93…`** (v4.06; there is no tag): GitHub's archive, checked by sha256, in
  `tests/tex-env/cache/` (git-ignored). `build` runs docstrip (`latex tuda-ci.ins`) in the
  image to make the `.cls`/`.sty` files from the `.dtx`.
- **A stand-in logo**: the TU logo cannot be redistributed, and the classes look for
  `tuda_logo.pdf` while the theme loads, before the `demo` option takes effect. `build` makes a grey
  bar of that name in `cache/tuda-ci/tex/`.

- **The font names database**, built once in the image: after the packages `install-texlive.sh` runs
  `luaotfload-tool --update --force` and looks `lmroman10-regular`, `XCharter` and `Roboto` up in it. Without that, every
  compile (a fresh `--rm` container each, up to 16 at once) would build the database itself in its first LuaLaTeX run,
  slowly, and that is where the lookups of fonts failed once (#327, #362). When a font is not found, the font packages
  (`lm lm-math xcharter roboto`) are installed again from a freshly resolved mirror and the database is built again; when
  that does not help the build fails and names the mirrors. `/etc/shale-tex-build` (and `status`) says so. The
  `tex_env` compile test asserts that no run builds the database.
- **Damaged mirrors**: a mirror in the middle of a sync makes `tlmgr` skip packages ("checksums differ", "downloading did
  not succeed") and still exit 0 (#362). `install-texlive.sh` greps its log, retries on a fresh mirror (three runs at most,
  like an older mirror), fails naming the skipped packages and the mirrors, and checks at the end that every package of
  `packages.txt` is installed. The retry of `env-tests.sh` stays on the font signature only: "package x is missing" is also
  what a real error in a template looks like, and the image can no longer come out incomplete unnoticed.

## The first build

It downloads about 94 MB of TeX Live packages and takes a few minutes. The image is about 470 MB.
After an edit of `packages.txt`, `build` runs the package step again.
