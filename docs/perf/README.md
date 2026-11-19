# Performance of Shale

How fast Shale is, measured the same way on every release (raba/shale#343, spec rows BUILD-027
to BUILD-035). This folder holds the stored results, the flamegraphs, the page
[`index.html`](index.html) that draws them, and below the how-to, the schemas and the **baseline
analysis**: what was measured before any optimisation, and the weak spots it found (the work order
for whoever fixes them).

## Running it

Everything needs a build with the cargo feature `bench`; no shipped build has it.

| What | Command |
|---|---|
| all scenarios (the `full` profile), result in `target/bench/<commit>.json` | `scripts/bench.sh` |
| a minute of it, small data (for development) | `scripts/bench.sh --profile quick` |
| the worst case (everything loaded at once) | `scripts/bench.sh --profile worst` |
| only scenarios whose ID contains a text; `=id` for exactly one | `scripts/bench.sh --filter ui.frame` |
| flamegraphs of every scenario of the profile (or of `--filter`) into `docs/perf/flamegraphs/` | `scripts/bench.sh --flamegraph` |
| two commits, built and run in temporary worktrees, then compared | `scripts/bench-compare.sh <old> <new>` |
| two result files | `shale --bench-compare old.json new.json --out dir` |
| the page `docs/perf/index.html` | `scripts/bench-report.sh` |

`scripts/bench.sh` builds with `--profile bench-release` (the release profile plus line tables, so
a flamegraph names functions and lines; `Cargo.toml`). Timings include the (small) cost of the
counting allocator. Run on a quiet computer: a second program on the same cores moves the numbers
by 10 % or more. `--iters N` sets the iterations of every scenario, `--min-ms MS` keeps measuring
until that much time has passed (flamegraphs use it).

**Flamegraphs** need `perf` (`kernel.perf_event_paranoid` of 2 or less lets it profile your own
processes) and `inferno` (`cargo install inferno`). Without them `bench.sh` says so and goes on.
Each scenario is profiled alone (`perf record -F 999 --call-graph dwarf`) and only the stacks
through the measured loop are kept: the data generation and the scenario's setup are not in the
picture. Reading one: the width of a box is the share of the time (the samples) spent in that
function **including everything it calls**; the stack grows upward, so the widest boxes right
above the bottom are where the time goes, and a wide box with little above it is time spent in
that function itself. Click a box to zoom, use the search to highlight a name. Look for a wide
plateau (one function doing a lot itself) or many thin towers (a loop that calls something
costly per item).

## Profiles

| Profile | What it runs | Use |
|---|---|---|
| `quick` | a part of the scenarios on small data (600 notes, 1500 messages), at most 5 iterations | landing, development: about a minute |
| `full` (default) | every scenario except `worst.*`, standard data (5000 notes, 20,000 messages, 5000 events, 3000 posts) | releases |
| `worst` | the `worst.*` scenarios: one window, everything loaded at once | releases |

Results of different profiles are never compared (the data differs): `--bench-compare` ends with
exit code 2.

**A release** (`scripts/make-release.sh`, via `scripts/bench-release.sh`) runs `full` and `worst`,
stores `docs/perf/results/vX.Y.Z.json` and `docs/perf/results-worst/vX.Y.Z.json`, compares each
with the highest earlier release that has one (`docs/perf/compare/vX.Y.Z.{md,json}`,
`vX.Y.Z-worst.{md,json}`), regenerates `index.html`, and puts all of it into the release commit.
A regression stops the release (`--allow-perf-regression` lets it pass, `--skip-bench` leaves the
step out). The step takes about ten minutes. A false alarm from a busy computer: run the release
again on a quiet one before using the flag.

## The scenarios

IDs are stable; the first word is the module or feature. `L` = the large vault (5000 notes), the
long note is 200 KB. "UI frame" scenarios run `ShaleApp::run_frame` headless in an egui context
(1500 x 900 points) and also tessellate the shapes, as the real window does before painting; only
the GPU upload and drawing are left out. A frame has 16.7 ms at 60 Hz.

| ID | What one iteration does |
|---|---|
| `vault.open.small/large` | `Vault::open`: walk the folder, read and index every note |
| `vault.rescan.large` | `Vault::rescan` of an open, unchanged vault (what the 3-second timer does) |
| `vault.check.large`, `vault.check.problems` | the vault check for other systems (`vault_check::check`) of L, and of a folder of 600 unportable names |
| `markdown.scan.long` | links, tasks, tags, headings, math, footnotes and fences of the long note |
| `editor.highlight.long`, `editor.layout.long` | the editor's layout job of the long note; and the galley egui makes from it |
| `search.sidebar.large` | `Vault::search`, the sidebar search of a common word |
| `search.all.large`, `.hit.large`, `.wild.large`, `.wild.hit.large` | `search_all::note_hits`: a word no note has (reads everything), a word most have (stops at the limit), and the same with a wildcard |
| `index.backlinks.large`, `index.unlinked.large`, `index.tasks.large` | backlinks and unlinked mentions of the most linked note; the task list and the tag list |
| `graph.build.large`, `graph.layout.small/large` | `Vault::graph`; the graph view's own build and 10 physics steps |
| `ui.frame.idle.large` | a window on L with no note open, no input |
| `ui.frame.sidebar.large` | the same with every folder of the file list open |
| `ui.frame.editor.long` / `ui.type.editor.long` / `ui.scroll.editor.long` | the long note open in the editor: idle, one typed letter in the middle, one scroll step |
| `ui.frame.backlinks.large` | the most linked note open with the backlinks panel |
| `ui.frame.graph.small/large`, `ui.frame.tasks.large` | the graph view; the task view |
| `ui.frame.drawing.large`, `drawing.scene.large` | a drawing of 2000 elements in the window; parsing and writing its JSON |
| `ui.frame.math.note`, `ui.frame.tables.note`, `ui.frame.code.note` | a note of 300 formulas, 60 tables, 120 code blocks |
| `ui.frame.reading.long` | the long note in the reading view |
| `picker.fuzzy.large` | the quick switcher: 3 typed prefixes scored against all 5000 names |
| `ui.type.vim.long`, `vim.keys.long` | vim mode: a key per frame in the long note; 40 vim commands on the text |
| `syntax.highlight.long`, `math.typeset.formulas`, `table.parse.long` | the highlighter on the fenced blocks of the long note (uncached); 200 formulas typeset; the table parser |
| `startup.app.large` | `ShaleApp::build` on L and 3 frames |
| `startup.app.recovery.large` | the same with a crash's leftovers waiting: recovered notes and half-written temp files |
| `startup.recovery.rescue`, `startup.recovery.unseen` | writing 300 rescued notes; finding the unseen days of the recovery folder |
| `import.docx.big` | 3 Word files of 1.5 MB text imported through pandoc into a vault (needs pandoc) |
| `pdf.export.note.long`, `pdf.export.book.large`, `pdf.export.book.docx` | Shale's own PDF writer: the long note; a folder of 100 notes as a book; the imported Word notes as a book |
| `pdf.render.book` | hayro renders 6 pages of the book |
| `pdf.tex.blocks.long`, `pdf.tex.engine` | the TeX export's parser (Shale's part); the engine run of its test document (the TeX program's time; skipped without `lualatex`) |
| `mail.sync.store.large` | what a sync does with the messages it fetched: save 20,000 messages and index them (the network is not in it) |
| `mail.open.large`, `mail.list.large`, `mail.search.large`, `mail.read.message`, `mail.index.rebuild.large` | opening the store; two pages of the list and the counts; full-text and wildcard search; reading 100 messages; rebuilding the index of 5000 messages |
| `ui.frame.mail.large` | the mail view with the 20,000-message inbox |
| `calendar.ics.parse.large`, `.write.large`, `calendar.store.load.large`, `calendar.occurrences.year.large`, `ui.frame.calendar.large` | 5000 events (a third repeating): parse, write, load a calendar, expand a year, the calendar view |
| `social.markup.html.large`, `social.mastodon.parse.large`, `social.bluesky.parse.large`, `social.index.add.large`, `social.index.search.large`, `ui.frame.social.large` | 3000 posts: HTML to Markdown; parsing both services' JSON; the local post index (add, search); the feed view |
| `git.sync.initial.large`, `git.sync.incremental.large`, `git.sync.pull.large` | `gitsync::sync` of L against a local bare repository: the first commit and push of 5000 notes; 50 edited notes; another computer pulling all of it |
| `git.lfs.sync.media` | six 4 MB files stored with git-LFS and uploaded (skipped without `git-lfs`) |
| `media.audio.decode` | decode and resample 60 s of audio (symphonia, rubato) |
| `recorder.encode` | 30 frames through the ffmpeg encoder of `--record` (skipped without `ffmpeg`) |
| `worst.startup`, `worst.frame.idle`, `worst.frame.typing`, `worst.views.cycle`, `worst.memory.growth` | the worst case below |

**The worst case** is one vault (L plus the Word files imported into it, a git repository with a
remote, a crash's leftovers in the recovery folder) with the 20,000-message inbox, the 5000-event
calendar and 3000 social posts loaded, the long note open beside its backlinks.
`worst.startup` starts the window on it; `worst.frame.idle` and `worst.frame.typing` are frames;
`worst.views.cycle` goes through every view (graph, tasks, mail, calendar, social, the note);
`worst.memory.growth` runs 1000 frames (typing, scrolling, view round every 100) twice, the
first as a warm-up, and reports in `retained_bytes` what the second left behind: steady growth
there is a leak or a cache without a limit. The result's `peak_rss_kb` of the `worst` profile is the
resident memory of the whole process.

### What is not measured, and why

Every module of `src/` that does work with data of its own has a scenario above. These are left
out on purpose:

- **The network**: `mail/imap_*`, `smtp_client`, `oauth`, `calendar/dav` (CalDAV), `social/mastodon`
  and `bluesky` calls, `import/tb_store` and `wl_drop` (Thunderbird, Wayland drops). Their pretend
  servers live in test-only code (they need a test-only certificate), and the time in them is the
  server's and the line's. What Shale does with the answers is measured (store, parse, index, view:
  `mail.*`, `calendar.*`, `social.*`). There is no `--stack` mode against the podman stacks of
  `tests/mail-env` and `tests/caldav-env`.
- **TeX**: the document writer needs the downloaded template sets, and the engine run needs
  `lualatex`: `pdf.tex.engine` is skipped without it, `pdf.tex.blocks.long` measures the parser.
- **The GPU and the system**: texture upload and drawing, audio output (`media/engine`, `output`:
  needs a sound card), video playback and `ffmpeg` cover art (`media/video`, `cover`), the clipboard
  and drag and drop (`import/wl_drop`, `app/drops`), system fonts (`fonts`), the keyring (`secrets`,
  `secret`), the instance lock and hand-over socket (`instance`), `opener`, `program`.
- **Dialogs and settings** (`dialog`, `settings*`, `data_usage`, `keys_ui`, `import/dialog`,
  `pdf/*_dialog`, `mail/dialog`, `mail/compose*`, `calendar/dialog`, `social/accounts_ui`): forms with
  no data of their own; a frame scenario draws them when they are open.
- **Small pure helpers** that a frame or another scenario already runs: `caption`, `colour_tags`,
  `cursor_style`, `embed_size`, `format`, `format_bar`, `key_badges`, `keys`, `layout`, `link_assist`,
  `link_preview`, `lists`, `theme`, `vault_path`, `jobs`, `script`.
- **`--screenshot`, `--record`, `--export-pdf`** as processes: `--export-pdf` is the same code as
  `pdf.export.*`, `--record` is `recorder.encode`.
- **Windows and macOS**: the harness runs on Linux (peak RSS is Linux only).

## The result files

`shale --bench out.json` writes (schema 1; `docs/perf/results/*.json` are such files):

```json
{
  "schema": 1,
  "commit": "full sha",  "describe": "v0.6.0 or git describe", "date": "2026-11-18T10:00:00Z",
  "host": {"cpu": "…", "cores": 28, "os": "linux x86_64 6.x", "load": 3.2},   // load: one-minute load average at the end of the run, Linux
  "profile": "bench-release",           // the build profile (SHALE_BENCH_PROFILE), else release or debug
  "run": "full",                        // quick, full or worst
  "scenarios": [{"id": "vault.open.large", "iters": 5, "median_ns": 0, "p95_ns": 0, "min_ns": 0,
                 "allocs": 0, "alloc_bytes": 0, "peak_bytes": 0, "retained_bytes": 0}],
  "skipped": [{"id": "import.docx.big", "reason": "pandoc is not installed …"}],
  "peak_rss_kb": 123456                 // VmHWM of the process, null off Linux
}
```

`allocs` and `alloc_bytes` are medians per iteration; `peak_bytes` is the largest of the
iterations (live bytes above the start of the iteration); `retained_bytes` is live bytes after the
last iteration minus before the first (what the iterations left behind; negative: they freed
memory). `SHALE_BENCH_COMMIT` and `SHALE_BENCH_DESCRIBE` set the header when the run is not inside
the repository.

## Comparing (for agents and releases)

`shale --bench-compare x.json y.json [--threshold 10] [--bytes-threshold 15] [--out dir]`, X the
older run, Y the newer one. Writes `dir/compare.md` (a table, the last line is the summary),
`dir/compare.json`, prints the summary line and one `REGRESSION <id> <metric> <delta>` line per
regression, and exits **0** (no regression), **1** (a regression above a threshold) or **2** (an
error: a missing or unreadable file, a schema other than 1, results of two profiles). When a computer was busy during a run (load above
half its cores) `compare.md` and the printed summary say so: times are then larger and less steady,
and a regression under 30 % deserves a second run on a quiet computer.

```json
{"schema": 1, "x": {"commit": "", "describe": "", "date": "", "load": 3.2, "cores": 28}, "y": {…},
 "threshold_time_pct": 10, "threshold_bytes_pct": 15,
 "rows": [{"id": "", "status": "both|added|removed",
           "x_median_ns": 0, "y_median_ns": 0, "time_delta_pct": 0.0, "time_verdict": "ok|faster|slower|REGRESSION|-",
           "x_peak_bytes": 0, "y_peak_bytes": 0, "bytes_delta_pct": 0.0, "bytes_verdict": "…"}],
 "regressions": [{"id": "", "metric": "time|peak_bytes", "x": 0, "y": 0, "delta_pct": 0.0}],
 "summary": {"compared": 0, "faster": 0, "slower": 0, "regressions": 0, "added": 0, "removed": 0}}
```

Verdicts: within 5 % is `ok`, beyond it `faster` or `slower`, above the threshold `REGRESSION`;
a change under 20 µs (time) or 64 KiB (peak) is `ok` whatever its percentage. A scenario only one
file has is `added` or `removed`: not an error. `scripts/bench-compare.sh <old> <new>` does the
whole round (worktrees, builds, runs, compare) and passes the exit code on; a commit older than
the harness is exit 2 ("has no benchmark harness").

A scenario's body may change with the harness (a commit that changes what a scenario does shows
as a difference in it, as `mail.sync.store.large` did when its message generation moved out of the
timed part): compare two commits that have the same harness, or read such a row with that in mind.

To check a fix: `scripts/bench-compare.sh <commit before the fix> HEAD --profile quick` (or `full`
for the scenarios `quick` does not have), then read `compare.md` for the scenario you changed and
for anything that got slower. `shale --bench out.json --filter <id> --iters 20` measures one
scenario quickly.

## Baseline: how it is now

Made by `scripts/bench.sh` (`--profile full`) and `scripts/bench.sh --profile worst` on commit
`4ec690f131` (`v0.6.0-16-g4ec690f`), 2026-11-18, build profile `bench-release`
(`docs/perf/results/baseline-4ec690f.json`, `docs/perf/results-worst/baseline-4ec690f.json`).
Host: Intel(R) Core(TM) i7-14700KF, 28 threads, linux x86_64 7.3.4-1-cachyos, **load average 7.31** at the end
of the `full` run (28.89 at the end of `worst`).

> **A shared computer.** Other builds and tests ran on the same 28 threads while these were
> measured (see the load averages above). Frame scenarios repeated within about 25 %, the scenarios
> that write files or start programs (git, mail, import, vault open) within a factor of two between
> runs on this computer; the allocation counts and peak bytes do not depend on the load at all (the
> same on every run). `Min` is the steadier of the two time columns. The baseline is for ranking
> and for the order of magnitude; for "did my fix help" run `scripts/bench-compare.sh` on a quiet
> computer and trust only differences above 30 %.

Time budgets: one frame at 60 Hz is **16.7 ms**; a click should answer within 100 ms; a start
within a second or two. `Allocs` and `Allocated` are per iteration; `Kept` is `retained_bytes`
(what the iterations left alive; see the file schema). Peak resident memory of the whole `full` run:
354 MiB; of the `worst` run: 671 MiB.

### Every scenario (full profile)

| Scenario | Median | Min | Allocs | Allocated | Peak | Kept |
|---|---:|---:|---:|---:|---:|---:|
| `vault.open.small` | 7.1 ms | 7.0 ms | 46 k | 6.8 MiB | 2.2 MiB | 640 B |
| `vault.open.large` | 154.9 ms | 154.0 ms | 931 k | 141.4 MiB | 47.9 MiB | 160 B |
| `vault.rescan.large` | 512.9 ms | 501.3 ms | 179 k | 8.6 MiB | 1.4 MiB | 320 B |
| `markdown.scan.long` | 1.8 ms | 1.7 ms | 4434 | 1.7 MiB | 234.8 KiB | 640 B |
| `editor.highlight.long` | 5.2 ms | 5.1 ms | 4464 | 13.9 MiB | 10.1 MiB | 640 B |
| `editor.layout.long` | 74.7 ms | 73.2 ms | 288 k | 150.3 MiB | 44.6 MiB | 3.9 MiB |
| `search.sidebar.large` | 89.4 ms | 88.9 ms | 35 k | 5.1 MiB | 4.3 MiB | 320 B |
| `search.all.large` | 101.5 ms | 101.2 ms | 5007 | 112.7 KiB | 33 B | 320 B |
| `search.all.hit.large` | 87.7 µs | 87.4 µs | 126 | 14.6 KiB | 8.5 KiB | 320 B |
| `search.all.wild.large` | 195.3 ms | 190.5 ms | 1.3 M | 564.7 MiB | 14.3 KiB | 160 B |
| `search.all.wild.hit.large` | 140.6 µs | 138.6 µs | 1205 | 518.0 KiB | 15.9 KiB | 320 B |
| `index.backlinks.large` | 26.8 ms | 26.6 ms | 349 k | 14.8 MiB | 1.8 MiB | 320 B |
| `index.unlinked.large` | 27.6 ms | 26.6 ms | 206 k | 35.7 MiB | 112.2 KiB | 160 B |
| `index.tasks.large` | 75.5 ms | 74.4 ms | 2.6 M | 103.1 MiB | 6.6 MiB | 320 B |
| `graph.build.large` | 24.1 ms | 23.9 ms | 405 k | 25.3 MiB | 6.0 MiB | 320 B |
| `graph.layout.small` | 4.4 ms | 4.3 ms | 20 k | 1.2 MiB | 280.2 KiB | 320 B |
| `graph.layout.large` | 778.2 ms | 752.2 ms | 405 k | 26.2 MiB | 6.0 MiB | 96 B |
| `ui.frame.idle.large` | 110.6 ms | 107.7 ms | 2.8 M | 139.2 MiB | 5.8 MiB | -38.7 MiB |
| `ui.frame.sidebar.large` | 132.0 ms | 128.4 ms | 2.8 M | 145.1 MiB | 7.0 MiB | 960 B |
| `ui.frame.editor.long` | 189.9 ms | 130.4 ms | 2.8 M | 171.3 MiB | 18.7 MiB | 960 B |
| `ui.frame.backlinks.large` | 470.6 ms | 446.4 ms | 3.5 M | 174.0 MiB | 5.8 MiB | 960 B |
| `ui.type.editor.long` | 161.3 ms | 158.5 ms | 2.8 M | 198.8 MiB | 21.5 MiB | 2.5 MiB |
| `ui.scroll.editor.long` | 133.8 ms | 131.7 ms | 2.7 M | 173.5 MiB | 18.7 MiB | 3.8 KiB |
| `ui.frame.graph.small` | 3.8 ms | 3.7 ms | 71 k | 11.2 MiB | 4.3 MiB | 960 B |
| `ui.frame.graph.large` | 312.9 ms | 309.0 ms | 2.5 M | 191.5 MiB | 47.9 MiB | -42.7 MiB |
| `ui.frame.tasks.large` | 566.5 ms | 541.0 ms | 6.0 M | 263.5 MiB | 5.8 MiB | 960 B |
| `vault.check.large` | 3.1 ms | 3.0 ms | 57 k | 2.4 MiB | 26.9 KiB | 320 B |
| `vault.check.problems` | 617.5 µs | 597.9 µs | 14 k | 796.4 KiB | 341.4 KiB | 640 B |
| `startup.recovery.rescue` | 2.2 ms | 2.1 ms | 6931 | 2.8 MiB | 6.3 KiB | 160 B |
| `startup.recovery.unseen` | 554.7 µs | 547.6 µs | 1857 | 119.5 KiB | 19.5 KiB | 640 B |
| `startup.app.large` | 1.25 s | 1.25 s | 25.3 M | 1.48 GiB | 96.2 MiB | 96 B |
| `startup.app.recovery.large` | 1.25 s | 1.25 s | 25.2 M | 1.48 GiB | 96.2 MiB | 96 B |
| `syntax.highlight.long` | 2.1 ms | 2.0 ms | 36 k | 1.1 MiB | 16.2 KiB | 10.6 KiB |
| `math.typeset.formulas` | 887.9 µs | 839.5 µs | 14 k | 892.3 KiB | 339.0 KiB | 1.3 MiB |
| `table.parse.long` | 2.7 ms | 2.6 ms | 32 k | 13.9 MiB | 129.5 KiB | 640 B |
| `vim.keys.long` | 2.7 ms | 2.6 ms | 77 | 34.2 MiB | 801.4 KiB | 640 B |
| `drawing.scene.large` | 2.1 ms | 2.1 ms | 13 k | 5.4 MiB | 2.8 MiB | 320 B |
| `ui.frame.drawing.large` | 107.4 ms | 105.8 ms | 2.7 M | 156.5 MiB | 8.5 MiB | 640 B |
| `ui.frame.math.note` | 1.2 ms | 1.1 ms | 6986 | 3.8 MiB | 1.7 MiB | 640 B |
| `ui.frame.tables.note` | 993.5 µs | 961.8 µs | 6814 | 2.2 MiB | 776.7 KiB | 640 B |
| `ui.frame.code.note` | 1.4 ms | 1.4 ms | 7916 | 4.0 MiB | 1.1 MiB | 640 B |
| `ui.type.vim.long` | 131.9 ms | 129.8 ms | 2.7 M | 174.7 MiB | 18.7 MiB | 832 B |
| `ui.frame.reading.long` | 124.1 ms | 122.6 ms | 3.0 M | 157.9 MiB | 7.0 MiB | 640 B |
| `picker.fuzzy.large` | 1.1 ms | 1.1 ms | 30 k | 431.1 KiB | 39 B | 640 B |
| `media.audio.decode` | 69.2 ms | 68.3 ms | 2452 | 10.3 MiB | 156.0 KiB | 160 B |
| `recorder.encode` | 164.2 ms | 162.9 ms | 112 | 107.4 KiB | 102.0 KiB | 96 B |
| `mail.sync.store.large` | 1.30 s | 1.30 s | 2.8 M | 302.9 MiB | 995.9 KiB | 80 B |
| `mail.open.large` | 239.0 ms | 237.7 ms | 160 k | 12.0 MiB | 4.4 MiB | 160 B |
| `mail.list.large` | 9.7 ms | 9.5 ms | 3093 | 366.1 KiB | 105.9 KiB | 640 B |
| `mail.search.large` | 24.6 ms | 24.4 ms | 809 | 91.9 KiB | 26.5 KiB | 320 B |
| `mail.read.message` | 664.9 µs | 657.3 µs | 13 k | 1.5 MiB | 17.6 KiB | 320 B |
| `mail.index.rebuild.large` | 443.1 ms | 443.1 ms | 698 k | 85.5 MiB | 1.3 MiB | 4.8 KiB |
| `ui.frame.mail.large` | 2.1 ms | 2.0 ms | 68 k | 3.5 MiB | 397.9 KiB | 960 B |
| `calendar.ics.parse.large` | 8.7 ms | 8.6 ms | 211 k | 17.8 MiB | 10.0 MiB | 320 B |
| `calendar.ics.write.large` | 15.5 ms | 15.4 ms | 434 k | 22.9 MiB | 3.9 MiB | 320 B |
| `calendar.store.load.large` | 8.9 ms | 8.8 ms | 211 k | 19.3 MiB | 11.6 MiB | 320 B |
| `calendar.occurrences.year.large` | 59.6 ms | 59.3 ms | 138 k | 11.6 MiB | 4.9 MiB | 160 B |
| `ui.frame.calendar.large` | 17.3 ms | 17.2 ms | 151 k | 7.1 MiB | 1021.4 KiB | 960 B |
| `social.markup.html.large` | 7.9 ms | 7.8 ms | 232 k | 11.8 MiB | 2.2 KiB | 320 B |
| `social.mastodon.parse.large` | 7.4 ms | 7.3 ms | 201 k | 10.9 MiB | 2.5 KiB | 320 B |
| `social.bluesky.parse.large` | 1.4 ms | 1.4 ms | 39 k | 2.5 MiB | 1.3 KiB | 320 B |
| `social.index.add.large` | 111.9 ms | 111.2 ms | 158 k | 21.9 MiB | 3.6 KiB | 96 B |
| `social.index.search.large` | 3.2 ms | 3.2 ms | 1540 | 305.6 KiB | 77.5 KiB | 320 B |
| `ui.frame.social.large` | 39.7 ms | 39.1 ms | 336 k | 49.1 MiB | 1.7 MiB | 960 B |
| `import.docx.big` | 3.55 s | 3.52 s | 4233 | 19.8 MiB | 4.9 MiB | 64 B |
| `pdf.export.note.long` | 113.1 ms | 112.5 ms | 767 k | 87.7 MiB | 8.9 MiB | 160 B |
| `pdf.export.book.large` | 174.8 ms | 174.3 ms | 1.3 M | 161.4 MiB | 16.3 MiB | 96 B |
| `pdf.export.book.docx` | 1.02 s | 1.02 s | 9.6 M | 959.4 MiB | 79.7 MiB | 96 B |
| `pdf.render.book` | 10.9 ms | 10.9 ms | 11 k | 50.9 MiB | 16.9 MiB | 96 B |
| `pdf.tex.blocks.long` | 3.3 ms | 3.2 ms | 41 k | 10.6 MiB | 734.9 KiB | 640 B |
| `git.sync.initial.large` | 763.0 ms | 763.0 ms | 12 k | 1.6 MiB | 287.2 KiB | 32 B |
| `git.sync.incremental.large` | 102.5 ms | 102.0 ms | 14 k | 1.7 MiB | 33.2 KiB | 160 B |
| `git.sync.pull.large` | 250.6 ms | 250.6 ms | 20 k | 2.5 MiB | 614.9 KiB | 32 B |
| `git.lfs.sync.media` | 1.30 s | 1.30 s | 12 k | 1.1 MiB | 39.2 KiB | 32 B |

Skipped here (no failure):

- `pdf.tex.engine`: no TeX for the engine run: the TeX of this computer: no lualatex in PATH

### The worst case (worst profile)

| Scenario | Median | Min | Allocs | Allocated | Peak | Kept |
|---|---:|---:|---:|---:|---:|---:|
| `worst.startup` | 2.55 s | 2.55 s | 55.0 M | 3.15 GiB | 131.1 MiB | 96 B |
| `worst.frame.idle` | 136.3 ms | 133.1 ms | 2.8 M | 176.9 MiB | 19.1 MiB | 1.9 KiB |
| `worst.frame.typing` | 163.3 ms | 159.0 ms | 2.8 M | 203.4 MiB | 21.9 MiB | 2.5 MiB |
| `worst.views.cycle` | 3.67 s | 3.63 s | 40.2 M | 2.87 GiB | 216.6 MiB | 118.3 MiB |
| `worst.memory.growth` | 561.08 s | 561.08 s | 3376.1 M | 219.35 GiB | 190.6 MiB | 510.9 KiB |

`worst.memory.growth` runs 1000 frames after one warm-up run of the same 1000; its `Kept` is what
the second run left behind.

## What the baseline says: the weak spots, in the order to fix them

Ranked by what a person feels: a frame over 16.7 ms is a stutter, over 100 ms is a freeze. The
flamegraph of every scenario is in [`flamegraphs/`](flamegraphs/) (`<id>.svg`; see
[`index.html`](index.html)); the percentages below are shares of the measured loop of the named
scenario, read from them (`perf`, frame pointers, a software clock; sums over 3 s or more).
`file:line` is the definition of the function to change.

**The headline: with a 5000-note vault every frame of the window costs 110.6 ms with nothing
happening** (`ui.frame.idle.large`; 2.8 M allocations per frame, about seven times the budget of the whole
frame; 110 to 150 ms on every run), and **starting Shale on it takes
1.25 s** (`startup.app.large`, 25.3 M allocations, 96.2 MiB peak; the worst case
2.55 s). A typed letter in a 200 KB note takes 161.3 ms, a scroll step
133.8 ms, the graph view 312.9 ms a frame, the task view 566.5 ms. The causes of the window-wide cost
are few, and all but the graph are the same two mistakes: **work that depends on the whole vault is
redone every frame** (PERF-001 says it must not be) and **paths are compared and sorted as
strings and as `Path` components** (PERF-002). Everything below is measured, not guessed.

| # | Weak spot | Where (file:line) | Evidence | Fix |
|---|---|---|---|---|
| 1 | **The status bar counts the open tasks every frame**: `all_tasks()` clones every task of every note and sorts them with a comparator that builds two lower-cased path strings per comparison | `src/layout.rs:951` calls `Vault::all_tasks` `src/vault.rs:711` (comparator `:713`) | 37 to 60 % of every window frame at 5000 notes: idle 53 %, editor 45 %, typing 39 %, sidebar 52 %, drawing 53 %, the first frame of start-up 61 %. `index.tasks.large` is the same call: 75.5 ms, 2.6 M allocations | Count the open tasks once per `vault.generation` (PERF-001); never sort to count; where a sorted list is needed sort with `sort_by_cached_key` or by an index, not by allocating in the comparator (the same comparator is in `src/vault.rs:393`, `:649` and `:690`) |
| 2 | **Backlinks and unlinked mentions of the open note are computed every frame** (right panel and status bar), each reading the text of every note | `src/panels.rs:514` `Vault::backlinks` `src/vault.rs:632`, `src/panels.rs:515` `Vault::unlinked_mentions` `:654`, `src/layout.rs:932` (`backlinks().len()` for the status bar) | `unlinked_mentions` 16 to 19 % of an idle frame, a note with few links; backlinks 26.8 ms and unlinked 27.6 ms per call in `index.backlinks.large`/`index.unlinked.large` | Compute per (note, `vault.generation`) and keep the result; unlinked mentions could be searched lazily when the panel section is opened |
| 3 | **Looking up a file by path is a linear search over `PathBuf == &Path`**, which compares component by component, and it is called per row | `Vault::file` `src/vault.rs:547` (called from `src/panels.rs:625` `mention_block`, `:416`, `:687`, `:717`) | a note with many backlinks (`ui.frame.backlinks.large`, 470.6 ms a frame): 56 % in `mention_block`, 48 % of it `PathBuf::eq`. The task view `ui.frame.tasks.large` (566.5 ms) pays the same per task group: 48 % in `Components::next_back` | A `HashMap<PathBuf, usize>` of the files built with `rebuild_name_index` (`src/vault.rs:460`) |
| 4 | **The 3-second rescan runs on the window's thread and has a quadratic check** | `src/app.rs:2726` calls `ShaleApp::rescan` `:1352`, `Vault::rescan` `src/vault.rs:381`; the `any(\|o\| o.rel == f.rel)` check at `:401` is the quadratic part, the sort at `:393` allocates per comparison | `vault.rescan.large` 512.9 ms (15 to 20 % of the time of every frame scenario, a stall of that length every 3 s; 95 % of the scan is path comparison) | A `HashSet` of the old paths (PERF-002), `sort_by_cached_key`, and the scan as a background job (JOB-001) that hands over its result |
| 5 | **The editor rebuilds the layout job of the whole note every frame**, though the text did not change | `src/editor.rs:1712` (the `layouter` closure: `highlight_with_math` `:268`, `make_room_for_numbers` `:494`) | `ui.frame.editor.long` 189.9 ms and `ui.type.editor.long` 161.3 ms: the editor is 14 to 25 % of the frame, 9 to 15 % in the layouter itself; the galley (egui's text layout, `editor.layout.long` 74.7 ms) is what a keystroke must pay, but a frame without an edit should pay nothing | Keep the `LayoutJob`/galley by (text hash, width, fonts, preview range) and rebuild only when one changes; lay out only the visible part of a 200 KB note |
| 6 | **The graph's physics is quadratic and runs three steps every frame** | `GraphView::step` `src/graph.rs:124`, called three times per frame at `:193` | `ui.frame.graph.large` 312.9 ms (63 % in `step`); `graph.layout.large` 778.2 ms for 10 steps (97 %); PERF-003 already says "a few hundred notes" | A grid or Barnes-Hut repulsion, steps bounded by a time budget, stop when the energy is low (`:193` keeps going while it is above 0.05), a smaller sample for the local graph |
| 7 | **Opening a vault parses every note on one thread, and the window waits for it** | `Vault::open` `src/vault.rs:356` → `rescan` → `reparse` `:520`: `markdown::find_links` `src/markdown.rs:161` (31 %), `find_tags` `:363` (28 %), `find_tasks` `:254` (11 %), `inline_code_ranges` `:104` (10 %), `Vault::resolve` (6 %) | `vault.open.large` 154.9 ms (931 k allocations, 47.9 MiB); `startup.app.large` 1.25 s (its first frame is 61 % fix 1; 5 frames are in it) | Parse on worker threads (the notes are independent), open the window first and index in the background (JOB-001), scan each line once for links, tags, tasks and code spans instead of four times |
| 8 | **Search over the vault is a lower-casing scan of every line, and it runs again every frame while a query is typed** | `Vault::search` `src/vault.rs:731` (`markdown::find_ci` `src/markdown.rs:991`: 80 %), called per frame at `src/panels.rs:339`; `search_all::wildcard_find` `src/search_all.rs:66` builds a `Vec<(usize, char)>` per line (41 %) | `search.sidebar.large` 89.4 ms; `search.all.large` (a word no note has) 101.5 ms; with a wildcard 195.3 ms and 1.3 M allocations; a word most notes have stops at the limit of hits: 87.7 µs | Search on the key press (not every frame), on a worker thread; lower-case each note once when it is indexed; no allocation per line |
| 9 | **The task view builds and lays out every task every frame** | `src/panels.rs:644` (`all_tasks()` again, 1), per-row `v.file` (3) | `ui.frame.tasks.large` 566.5 ms, 6.0 M allocations | 1 and 3, then `ScrollArea::show_rows` (only the visible rows) |
| 10 | **The calendar view recomputes the occurrences every frame** | `CalendarStore::occurrences` `src/calendar/store.rs:450` sorts `Occ` (50 % in the sort) over `ics::occurrences` `src/calendar/ics.rs:613` (`localtime_r` per occurrence) | `calendar.occurrences.year.large` 59.6 ms; `ui.frame.calendar.large` 17.3 ms a frame | Cache per (`generation`, range); `sort_by_cached_key`; one time-zone lookup per event, not per occurrence |
| 11 | **Opening the mail store checks the whole index every time** | `db::quick_check` `src/mail/db.rs:180` (SQLite's `quick_check` with the FTS integrity check), called from `prepare_index` `src/mail/store.rs:660` (`MailStore::open` `:211`), and `reconcile` `src/mail/store.rs:703` | `mail.open.large` 239.0 ms for 20,000 messages: 73 % `quick_check`, 26 % `reconcile`; grows with the inbox | Check only after an unclean shutdown or on request, `reconcile` from the directory's change time; it is on a background job already, so the window does not freeze, but the inbox is empty for that long |
| 12 | **Feed view: thousands of posts are laid out each frame** | `ShaleApp::social_view` `src/app/social.rs:16` (91 % of the frame) | `ui.frame.social.large` 39.7 ms for 3000 posts (336 k allocations); `social.index.add.large` 111.9 ms for 3000 posts (the FTS writes: `src/social/post_index.rs:115`) | Virtualise the list (`show_rows` with measured heights); batch the index writes off the window's thread |
| 13 | **Memory that stays**: typing keeps about 86 KB per key in a 200 KB note; a round through the views keeps tens of MB the first times | typing: the undo history (suspect: `History::checkpoint` `src/undo.rs:119`, which keeps a copy of the note and the steps of the edits); views: not found yet | `ui.type.editor.long` kept 2.5 MiB in 30 keys; `worst.frame.typing` 2.5 MiB; `worst.views.cycle` kept 118.3 MiB in 3 rounds, yet `worst.memory.growth` 510.9 KiB in 1000 frames with 10 rounds: it is a first-visit cost, not a leak, but 40 MB per round is worth a look (suspects: the graph, the mail list, the feed) | Store undo steps as diffs; find what the first visit of a view keeps with `retained_bytes` per view (add scenarios) |
| 14 | **PDF export spends a third of its time compressing** | `deflate` `src/pdf/write.rs:443` (`compress_to_vec_zlib(…, 6)`), `Page::text` | `pdf.export.note.long` 113.1 ms (38 % `miniz_oxide`), `pdf.export.book.docx` 1.02 s (9.6 M allocations, 79.7 MiB) | Level 1 to 3 for page streams, compress on worker threads; fewer allocations per drawn line |

Smaller things the numbers show:

- `search.sidebar.large`, `index.*`, `graph.build.large` (24.1 ms) and the sorts in 1 all call
  `rel_to_string(..).to_lowercase()` in a comparator; one `sort_by_cached_key` helper fixes them all.
- `startup.app.recovery.large` is not slower than `startup.app.large`: a crash's leftovers cost
  nothing at start-up (`startup.recovery.*` are 2.2 ms and 554.7 µs). The vault check
  is 3.1 ms for 5000 notes.
- Fine as they are (well under a frame): `markdown.scan.long` 1.8 ms, `ui.frame.math.note`
  1.2 ms, `ui.frame.tables.note` 993.5 µs, `ui.frame.code.note` 1.4 ms, `ui.frame.mail.large` 2.1 ms (the
  list is virtualised), `mail.list.large` 9.7 ms, `mail.search.large` 24.6 ms (SQLite FTS), `vim.keys.long`
  2.7 ms, `table.parse.long` 2.7 ms, `syntax.highlight.long` 2.1 ms, `pdf.render.book` 10.9 ms, `calendar.ics.parse.large`
  8.7 ms, `social.*.parse.large`, `media.audio.decode` 69.2 ms for 60 s of sound.
- Time that is not Shale's: `import.docx.big` (3.55 s) is pandoc, `git.sync.*` and `git.lfs.sync.media` are
  git, `recorder.encode` is ffmpeg; their `allocs` are the harness's own glue.
- `mail.sync.store.large` (1.30 s for 20,000 messages, 2.8 M allocations) is the MIME parse
  for the search index (`shale::mail::parse` 42 %) and a file per message; it runs in the sync job.

### How to use this list

1. Fix 1, 2, 3 and 4 first: they are small, they are the same kind of change (a cache keyed by
   `vault.generation`, a path index, no allocation in comparators) and together they should take the
   idle frame of a 5000-note vault from 110.6 ms to a few milliseconds; `ui.frame.idle.large`,
   `ui.frame.sidebar.large`, `ui.frame.editor.long`, `ui.type.editor.long`, `ui.scroll.editor.long`,
   `ui.frame.backlinks.large`, `ui.frame.drawing.large`, `startup.app.large`, `vault.rescan.large`,
   `index.tasks.large` show it. Then 5 (the editor), 7 (start-up) and 6 (the graph).
2. After each fix: `scripts/bench-compare.sh <before> HEAD --profile quick`, then keep the faster
   numbers: a release stores them in `results/` and stops on a regression.
3. A new feature that can be slow with a lot of data gets a scenario in `src/bench/` (the ID
   rules and the data generators are there, `docs/spec/engineering.md` BUILD-028).
