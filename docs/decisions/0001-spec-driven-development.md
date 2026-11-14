# ADR-0001: Spec-driven development with IS NOW / SHOULD BE specs

- **Status**: Proposed
- **Date**: 2026-11-14

## Context

Shale has grown to about 22,500 lines in 38 modules: notes, links, drawings, tasks, a calendar
with CalDAV, e-mail and event import, people, automatic tags, and PDF export and rendering.
Features land in large steps, often one session each.

What the app *must* do is written down in three places, and none of them is a spec:

- `README.md` and `docs/FEATURES.md` describe features for users.
- `docs/IMPLEMENTATION.md` (270 lines) mixes an architecture table, a log of "rounds", a test
  inventory, a backlog and known issues. Rules such as the link resolution order or the autosave
  delay are buried in prose.
- The 179 inline `#[test]`s are the real specification, but no written requirement points to them.

This has already caused drift ([ADR-0002](0002-documentation-structure.md)). It has also hidden
real defects. The [review](README.md#review-2026-11-14) found that the promise "nothing is ever
lost" does not hold: a note that cannot be read opens empty and is overwritten, and an outside
change made while the user is typing is silently overwritten. Nobody noticed, because the promise
was never written down as a rule that a test checks.

## Decision

1. **Two kinds of documents, in two sibling folders**:
   - `docs/decisions/NNNN-title.md`: **decision records**. They are short and are not edited once
     accepted; a later ADR supersedes them.
   - `docs/spec/<area>.md`: **living specs**, edited with every behaviour change. See the
     [spec index](../spec/README.md).
2. Every spec rule has a **stable ID** `AREA-NNN`. IDs are never reused; a dropped rule is
   marked *Retired*.
3. Each rule is one **testable statement** with these fields:
   - **Status**:
     - `IS NOW`: true of the code today. This includes known defects, written as they are and
       marked ⚠.
     - `SHOULD BE`: an agreed target that is not true yet.
   - **Code**: where it is implemented.
   - **Test**: the tests that check it, or `untested`.
4. A `SHOULD BE` that needs a real design choice gets an ADR. Questions that are not decided yet
   go to [open points](../spec/open-points.md) and are not ADRs.
5. **Traceability**: a test that checks a rule carries the rule's ID in its name or in a
   `// spec: LINK-003` comment. Existing tests are tagged when they are touched; there is no
   big-bang retagging. A known defect may get an `#[ignore]` test named after its SHOULD BE ID.
   Later, a unit test can check that every ID in the specs is used by a test or marked
   `untested` (OPEN-012).
6. **Workflow for a behaviour change**:
   1. Add or change the rule as `SHOULD BE`, plus an ADR if it is a real decision.
   2. Implement it and write the test that carries the ID.
   3. Flip the rule to `IS NOW`, fill in Code and Test, and add the user-facing line to
      `CHANGELOG.md`.
7. **What we spec**: data safety and persistence, file formats and round trips, link syntax,
   resolution and rewriting, where state lives, security boundaries (external programs, network,
   path containment), sync and conflict behaviour, and keyboard bindings. Each area has about
   5–15 rules.
8. **What we do not spec**: layout, colours and message wording, drawing UX details,
   heuristics such as Thunderbird lookups or tag learning (their reasoning goes into ADR
   context), how something is implemented, and exact timings (they appear only as bounds).

## Consequences

- One place answers "what must Shale do?", and reviews can check a change against it.
- Defects become visible: an `IS NOW ⚠` rule next to its `SHOULD BE` fix is a work list.
- Cost: specs must be kept current. The workflow makes the spec the *first* step, and rules are
  kept short.
- `docs/IMPLEMENTATION.md` shrinks to architecture and development notes, and the history moves
  to `CHANGELOG.md` and git ([ADR-0002](0002-documentation-structure.md)).
