---
name: lead
description: Orchestrate a batch of Shale issues from one session (plan, start one implementing subagent per issue in its own worktree, review, land with scripts/land.sh, clean up). Use when the owner hands over several issues at once, a label or milestone to work through, or says "lead", "orchestrate" or "work these issues".
---

# Lead session (FORGE-022)

You plan, start the subagents, review and land. You do not write the code yourself. Follow
`CLAUDE.md` throughout, above all "Sessions side by side" (FORGE-021) and FORGE-011.

## 1. Plan (in the main checkout, which stays on `main`)

1. `git fetch origin`; read each issue (`gh issue view N -R rabalyn/shale --comments`) and the spec
   rows it touches in `docs/spec/`.
2. Drop issues that another session already claimed (a comment or a `claude/*-N` branch,
   `git branch -a --list '*-N'`). Claim the rest with a comment naming the branch you will use.
3. Sort them into **independent** (different files or features: run at the same time) and
   **dependent** (same files, or one needs the other: run in order, the second branched from
   `origin/main` after the first landed). Two issues in the same area run in order even if they look
   independent: a conflict at landing costs more than the wait.
4. Show the owner the plan (issue, branch, order) when it has more than three issues or a
   dependency you guessed; otherwise go ahead.

## 2. Start one subagent per issue

The branch is `claude/<topic>-<N>` (the claim comment names it). How the worktree is made depends
on the tool:

- **Claude (desktop or Code)**: start the subagent with `isolation: "worktree"`. A worktree you
  made by hand is refused for new files ("belongs to a different worktree"), because the subagent
  is rooted in your own worktree (#550). The isolated worktree is fresh, so its prompt's first
  step is `git fetch origin && git checkout -B claude/<topic>-<N> origin/main` and a check that
  `git log -1` is the commit of `origin/main`. Its report names its worktree's absolute path and
  its branch.
- **opencode**, which has no such isolation: make the worktree yourself and give the subagent its
  absolute path:

  ```sh
  git worktree add .claude/worktrees/<topic>-<N> -b claude/<topic>-<N> origin/main
  ```

Start the subagent with the model the owner's instructions name for implementation work. It
starts with no context, so its prompt says:

- where it works: the isolated worktree (Claude), or the absolute path of the hand-made one
  (opencode); it works only there;
- the issue number and its text, and the spec rows by ID;
- "follow `CLAUDE.md` in the worktree": spec first, tests named after the rule, no warnings, docs,
  user guide, screenshots and a `docs/changelog-pending/I<N>.md` entry;
- portable code and tests (DEV-049 in `CLAUDE.md`): no hardcoded paths, `/tmp`, ports or tools of one system;
- commit with `Closes #N` (or `Refs #N` when the issue stays open), **do not run `land.sh`, do not
  push**, and report: the worktree path and branch, the commits, what was checked (`cargo fmt
  --check`, clippy, `cargo nextest run`), what is left open.

Start the independent ones together; do not poll them, wait for their reports.

## 3. Review each result

In the worktree path the subagent reported: `git log origin/main..HEAD`, `git diff origin/main...HEAD`. Check against the
issue and the spec rows: the rows flipped to IS NOW with real test names, every code path tested,
docs and changelog updated, no stray files. Send findings back to the **same** subagent (continue
its session) rather than fixing them yourself; small wording fixes you may do. A subagent that
finished may resend its report; stop it with the task-stop tool once its work is committed.

## 4. Land, one at a time

```sh
scripts/land.sh claude/<topic>-<N>
```

Only one landing at a time, across all sessions (another session may be landing: wait for it). On a
conflict, have the subagent merge `origin/main` into its branch and resolve it there (keep both
sides; renumber a clashing spec ID on its side), then land again. A red `main` is fixed first.
After a landing, the next dependent issue branches from the new `origin/main`.

## 5. Clean up and report

- For each landed branch, a hand-made worktree: `git worktree remove .claude/worktrees/<topic>-<N>`
  and `git branch -d claude/<topic>-<N>`.
- For an isolated worktree: `git worktree remove .claude/worktrees/agent-<id>`, then delete both
  branches, `claude/<topic>-<N>` and `worktree-agent-<id>`. The latter stays at the old base, so
  `git branch -d` works once it is in `origin/main`; check that first.
- Never remove a worktree with uncommitted changes (`git -C <path> status --short`) or one that is
  another process's working directory (FORGE-021); `pgrep -f` matches its own command line, so
  look for other hits than that.
- Comment on issues that stay open (what was done, what is left).
- Final report: per issue, landed / open / dropped and why, and the issues you opened, commented on
  or closed.
