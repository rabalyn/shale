# ADR-0024: Shale is licensed under the GPL-3.0-or-later

- **Status**: Accepted (owner request, 2026-11-19)
- **Date**: 2026-11-19
- **Specs**: BUILD-026 ([`docs/spec/engineering.md`](../spec/engineering.md)) names the licence of the AUR package
- **Issues**: #434

## Context

Shale was published under the MIT licence. MIT lets anyone take the code and build a closed product
from it, without giving anything back. The owner wants that ruled out. The owner is the sole
copyright holder, so the licence can be changed.

## Options

1. **Keep MIT**: no work, but a closed product made from Shale stays allowed.
2. **GPL-3.0-or-later**: anyone who distributes Shale or a work based on it must offer the source
   under the same terms. Open source by the usual definitions; distributions ship it.
3. **AGPL-3.0**: the GPL plus a clause for network use. A desktop app barely needs that clause, and
   many companies ban the AGPL outright.
4. **A noncommercial licence** such as PolyForm Noncommercial: not open source, distributions will
   not ship it, and "commercial" is fuzzy.

## Decision

Option 2, **GPL-3.0-or-later**, from this change on. `LICENSE` holds the GPL-3.0 text, `Cargo.toml`
and the AUR package name the SPDX identifier `GPL-3.0-or-later`.

## Consequences

- **Releases up to and including v0.6.7 stay MIT for good**; what was published under MIT cannot be
  taken back.
- **The dependencies fit.** All crates in `Cargo.lock` are GPL-3-compatible (MIT, Apache-2.0, BSD,
  Zlib, ISC, MPL-2.0, BSL-1.0, Unicode-3.0, CDLA-Permissive-2.0). The one crate that is
  "Apache-2.0 OR GPL-2.0-only" is used under Apache-2.0.
- The bundled fonts stay under OFL-1.1 and the theme colour schemes under MIT, as separate works with
  their own notices.
- As the sole copyright holder the owner may still offer Shale under commercial licences. External
  contributions would need a contributor licence agreement (CLA) to keep that possible.
- The GPL conflicts with Apple's App Store terms. Shale is not distributed there.
