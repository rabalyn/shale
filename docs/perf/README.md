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
| only scenarios whose ID contains a text; `=id` for exactly one; several, comma separated | `scripts/bench.sh --filter ui.frame` |
| flamegraphs of every scenario of the profile (or of `--filter`) into `docs/perf/flamegraphs/` | `scripts/bench.sh --flamegraph` |
| two commits, built and run in temporary worktrees (3 interleaved runs each), then compared | `scripts/bench-compare.sh <old> <new>` |
| two result files; several runs of one commit as one result | `shale --bench-compare old.json new.json --out dir`; `shale --bench-merge out.json run1.json run2.json` |
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

`shale --bench out.json` writes (schema 2; `docs/perf/results/*.json` are such files; schema 1 files are read too: their scenarios are version 1, one run):

```json
{
  "schema": 2,
  "commit": "full sha",  "describe": "v0.6.0 or git describe", "date": "2026-11-18T10:00:00Z",
  "host": {"cpu": "…", "cores": 28, "os": "linux x86_64 6.x", "load": 3.2},   // load: one-minute load average at the end of the run, Linux
  "profile": "bench-release",           // the build profile (SHALE_BENCH_PROFILE), else release or debug
  "run": "full",                        // quick, full or worst
  "scenarios": [{"id": "vault.open.large", "iters": 5, "median_ns": 0, "p95_ns": 0, "min_ns": 0,
                 "allocs": 0, "alloc_bytes": 0, "peak_bytes": 0, "retained_bytes": 0,
                 "version": 1,                  // bumped when the scenario's definition changes (see Comparing)
                 "run_medians_ns": []}],        // only in a merged result: the median of each run
  "repeats": 1,                         // how many runs the result merges (--bench-merge)
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

`shale --bench-compare x.json y.json [--threshold 10] [--bytes-threshold 15] [--allocs-threshold 10] [--changed ID[,ID]] [--out dir]`,
X the older run, Y the newer one. Writes `dir/compare.md` and `dir/compare.json`, prints the summary
line and one `REGRESSION <id> <metric> <delta>` line per regression (and a `NOISY` line per scenario
to run again), and exits **0** (`ok` or `faster`), **1** (`regression`) or **2** (`error`: a missing
or unreadable file, a schema this build does not read, results of two profiles; `compare.json` then
holds `{"schema": 2, "verdict": "error", "exit_code": 2, "error": "…"}`).

### How a verdict is made (BUILD-036)

Two kinds of numbers, weighed differently:

| Metric | Kind | Rule |
|---|---|---|
| allocations per iteration (`allocs`) | **exact**: the same on every run, busy computer or not | `REGRESSION` above +10 %; `more`/`fewer` beyond ±2 %; a difference under 500 allocations is `ok` |
| peak bytes (`peak_bytes`) | **exact** | `REGRESSION` above +15 %; `larger`/`smaller` beyond ±5 %; under 64 KiB is `ok` |
| median time (`median_ns`) | **noisy**: a busy computer only makes it larger | needs the difference above the noise band (5 %) and 20 µs **and** the *spreads of X and Y not to overlap*; then `REGRESSION` above +10 %, else `slower`/`faster`. A difference inside the spread is `noisy` |

A side's spread is the range of the medians of its runs (`--runs`, merged by `--bench-merge`; the
time of a merged side is its *smallest* median, the best the computer managed); a single run has
the range from its fastest to its 95th percentile iteration. So repeated, interleaved runs make a
time verdict trustworthy on a busy computer, and an allocation or peak regression is a clear signal
without any repeat. A time regression under 30 % while a computer was busy (load above half its
cores) has `confidence` `low`: run it again. More allocations or memory in a scenario that got clearly
faster (a quadtree for the graph) is a `tradeoff`, listed, not a regression. A scenario whose `version` differs between X and Y
(next section) is `changed`: no verdict. A scenario only one file has is `added` or `removed`.

**Scenario versions.** Every scenario has a `version` in the result file (1 unless a file says
otherwise; schema 1 files have none). When a change to the harness makes a scenario measure
something else (`calendar.occurrences.year.large` is at version 2: it shifts its year every
iteration, because the calendar store now keeps the ranges it made), the harness bumps the
version in its definition (`Scenario::version`, `src/bench/scen_data.rs`), and every comparison across
the change says `changed`. For results made before the field existed, name the scenarios:
`--changed id[,id]` (also on `scripts/bench-compare.sh`).

**Quiet threads.** The allocation counters are the process's. Before a scenario's warm-up and its
measured loop the harness waits until no thread allocates (under 50 allocations in 100 ms, at most
10 s): a background job that a dropped window left behind would otherwise be counted into the next
scenario (`vault.check.large` showed three times its allocations after a fast graph frame).

### compare.json

```json
{"schema": 2, "verdict": "ok|faster|regression|error", "exit_code": 0,
 "x": {"commit": "", "describe": "", "date": "", "load": 3.2, "cores": 28, "repeats": 3}, "y": {…},
 "busy": false,                       // a computer was busy during a run: times are less steady
 "threshold_time_pct": 10, "threshold_bytes_pct": 15, "threshold_allocs_pct": 10,
 "regressions":  [{"id": "", "metric": "time|allocs|peak_bytes", "x": 0, "y": 0, "delta_pct": 0.0, "confidence": "high|low"}],  // the worst first
 "improvements": [{…same…}],          // the best first
 "tradeoffs": [{…same…}],             // more allocations/peak, but the time is clearly better: not a regression
 "noisy": ["id"],                     // time moved more than the threshold, inside the spread: run again with --filter
 "changed": [{"id": "", "reason": "version|--changed", "x_version": 1, "y_version": 2}],
 "skipped": [{"id": "", "side": "x|y|both", "reason": ""}],
 "summary": {"compared": 0, "faster": 0, "slower": 0, "regressions": 0, "noisy": 0, "changed": 0, "added": 0, "removed": 0},
 "rows": [{"id": "", "status": "both|changed|added|removed",
           "x_median_ns": 0, "y_median_ns": 0, "time_delta_pct": 0.0, "time_verdict": "ok|faster|slower|noisy|REGRESSION|changed|-",
           "x_range_ns": [0, 0], "y_range_ns": [0, 0],
           "x_allocs": 0, "y_allocs": 0, "allocs_delta_pct": 0.0, "allocs_verdict": "ok|fewer|more|REGRESSION|tradeoff|changed|-",
           "x_peak_bytes": 0, "y_peak_bytes": 0, "bytes_delta_pct": 0.0, "bytes_verdict": "ok|smaller|larger|REGRESSION|tradeoff|changed|-",
           "x_version": 1, "y_version": 1}]}
```

`compare.md` is for people: the verdict, the regressions, the 15 best improvements, the noisy and
changed scenarios, the skipped ones, then the table of every scenario; the last line is the summary.

### scripts/bench-compare.sh

`scripts/bench-compare.sh <X> <Y> [--profile quick|full|worst] [--runs N] [--filter IDS] [--changed IDS] [--max-load L] [--flamegraphs] [--out DIR]`
does the whole round: a temporary worktree and a `bench-release` build per commit (the comparison
itself is made by the harness of the checkout the script is in, so X and Y may be older than
`--bench-merge`), then `--runs` runs of each side (default 3, `worst` 2) **interleaved** (X, Y, Y,
X, X, Y …: a busy minute hits both sides alike), `--bench-merge` per side, `--bench-compare`. The
output folder (default `target/bench-compare-out`) has `x.json`, `y.json`, `runs/`, `compare.md`,
`compare.json`. `--max-load L` waits (at most `--max-wait`, default 600 s) before each run until
the one-minute load average is below `L`. `--filter` takes IDs or parts of IDs, comma separated
(`=id` for exactly one): measure only those. With `--flamegraphs` it renders, for the regressed,
the noisy and the best improved scenarios (at most `--flame-max`, default 6), `flamegraphs/x/<id>.svg`,
`flamegraphs/y/<id>.svg` and `flamegraphs/diff/<id>.svg` (`inferno-diff-folded`: red is more
time in Y) in the output folder, never in git. The same exit codes as `--bench-compare`; a failure
before the comparison (a commit without the harness, a failed build or run) writes
`compare.json` with `verdict: "error"` and exits 2. Cost: three builds and `2 × runs` runs; the
`full` profile takes about ten minutes a run on a quiet computer, so give the whole thing the better
part of an hour (run it in the background).

## Agent playbook: a performance check in a release or after a fix

What an agent does, and in this order. Everything is a command; nothing needs a person.

1. **A release does it for you.** `scripts/make-release.sh` runs `scripts/bench-release.sh` in the
   release worktree: `full` twice (`BENCH_RUNS`, merged) and `worst` once (`BENCH_WORST_RUNS`),
   compared with the last release's stored result into `docs/perf/compare/vX.Y.Z.{md,json}` and
   `vX.Y.Z-worst.{md,json}` (the same `compare.json` as below). On a regression it ends with
   "performance regression" and the lines `<id> <metric> <x> -> <y> (<delta> %, confidence …)`.
   Go on at 4. `--allow-perf-regression` lets the release pass: only with the reason in the
   release issue (step 6), `--skip-bench` leaves the step out and says so.
2. **A fix or a branch** (is it faster, did it break something): `scripts/bench-compare.sh <before>
   <after> --profile quick` (about 15 minutes with the three runs; `full` for what `quick` lacks,
   `worst` for the worst case; `--max-load 12` on a shared computer).
3. **Read `compare.json`, and only that**: `jq '{verdict, regressions, noisy, changed, busy}' compare.json`.
   - `verdict` `ok` or `faster` and exit 0: done; `improvements` says what got better.
   - `error` (exit 2): `error` says why; fix the cause (a commit older than the harness, a
     failed build), do not retry blindly.
   - `regression` (exit 1): `regressions` is sorted, the worst first. An `allocs` or `peak_bytes`
     entry is certain (the numbers are exact). A `time` entry with `confidence` `high` has spreads
     that do not overlap, so it is very probably real; `low` means a busy computer and less than
     30 %: run it again (4).
   - `noisy` lists scenarios whose time moved over the threshold *inside* the spread: not a regression, but
     look at them again (4) before a release.
   - `changed` scenarios have no verdict by design; `skipped` ones lacked a tool (pandoc, ffmpeg,
     lualatex, git-lfs) on one side: not a failure.
4. **Run the flagged scenarios again, alone**, quiet if you can:
   `scripts/bench-compare.sh <X> <Y> --profile full --runs 5 --filter =id1,=id2 --max-load 8`
   (the profile of the flagged scenario: `worst.*` is `worst`). A regression that stays is real.
   A `noisy` one that vanishes was noise.
5. **Find the cause.** The same command with `--flamegraphs` writes `flamegraphs/x`, `y` and
   `diff` of the regressed scenarios into the output folder (needs perf, `inferno`, `jq`;
   `kernel.perf_event_paranoid` 2 or less). Open `diff/<id>.svg` first: the red boxes are code that
   takes more of the time in Y (read the flamegraph section above); then the Y flamegraph for the
   whole stack and `git diff <X>..<Y> -- <file of that function>`. Do not commit these SVGs.
6. **Open an issue with the numbers** (`gh issue create -R rabalyn/shale`, label `performance` when it
   exists): the scenario, the metric, X and Y values and the delta (from `compare.json`), the two commits,
   the number of runs and the load (`x.load`, `y.load`), what the diff flamegraph shows (function names,
   `file:line`), and the command that reproduces it. For a release, say in it that and why the
   release went on with `--allow-perf-regression`, or fix first and release again (a red check is
   fixed in a branch, landed, then the next release is made; never tag by hand).
7. **After the fix**, step 2 again against the commit before it; keep the numbers in the issue's
   closing comment.

Rules of thumb: trust `allocs` and `peak_bytes` anywhere; trust a `time` verdict from three or more
interleaved runs; never trust a single run on a computer with a load average above half its cores;
never compare across a `changed` scenario or across profiles.

To check a fix quickly: `shale --bench out.json --filter <id> --iters 20` measures one scenario.

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

## Before/after (phase 2)

Three results: the **baseline** (`4ec690f`, above, a single run), **X** = `c82f693` (the harness
merged with the then-current `main`, before the fixes of #377 to #386; it already had the editor
cache of #372) and **Y** = `4858dc5` (the fixes, plus the benchmark tooling of BUILD-036/037). X and Y
were run three times each (`worst`: twice), interleaved, with `scripts/bench-compare.sh c82f693
4858dc5` on a shared 28-thread computer (load average 3 to 11 before a run; 22.9 and 16.7 at the end
of the `full` runs of X and Y, so the times carry the spread of the runs: the full tables are
[`compare/c82f693-4858dc5.md`](compare/c82f693-4858dc5.md) and `-worst.md`; `.json` for tools; stored
results `results/c82f693.json`, `results/4858dc5.json` and the same in `results-worst/`). Medians
(the best of the runs); allocations per iteration; the verdict is that of `compare.json`.

| Scenario | Baseline | X | Y | Allocations X -> Y | Verdict (time / allocs) |
|---|---:|---:|---:|---:|---|
| `ui.frame.idle.large` | 110.59 ms | 30.64 ms | **0.75 ms** | 208,909 -> 2,698 | faster / fewer |
| `ui.frame.sidebar.large` | 131.95 ms | 47.47 ms | 17.38 ms | 267,905 -> 61,694 | faster / fewer |
| `ui.frame.editor.long` | 189.95 ms | 31.72 ms | **1.57 ms** | 208,033 -> 3,845 | faster / fewer |
| `ui.type.editor.long` | 161.28 ms | 124.73 ms | 30.51 ms | 2,835,784 -> 150,303 | faster / fewer |
| `ui.scroll.editor.long` | 133.81 ms | 31.75 ms | **1.60 ms** | 208,162 -> 3,969 | faster / fewer |
| `ui.frame.backlinks.large` | 470.61 ms | 357.18 ms | **10.07 ms** | 606,655 -> 52,527 | faster / fewer |
| `ui.frame.tasks.large` | 566.46 ms | 485.71 ms | **1.15 ms** | 3,491,247 -> 4,235 | faster / fewer |
| `ui.frame.graph.small` | 3.80 ms | 1.71 ms | **2.43 ms** | 2,449 -> 5,455 | REGRESSION / REGRESSION |
| `ui.frame.graph.large` | 312.91 ms | 230.13 ms | 25.25 ms | 2,328 -> 17,934 | faster / tradeoff |
| `ui.frame.drawing.large` | 107.38 ms | 35.16 ms | **4.75 ms** | 227,559 -> 21,353 | faster / fewer |
| `ui.frame.reading.long` | 124.08 ms | 34.46 ms | **4.32 ms** | 246,634 -> 42,446 | faster / fewer |
| `ui.frame.calendar.large` | 17.30 ms | 14.72 ms | **0.82 ms** | 82,658 -> 3,154 | faster / fewer |
| `ui.frame.social.large` | 39.73 ms | 35.37 ms | **0.81 ms** | 267,283 -> 11,065 | faster / fewer |
| `ui.frame.math.note` / `tables.note` / `code.note` | 1.15 / 0.99 / 1.41 ms | 0.47 / 0.44 / 0.50 ms | 0.19 / 0.15 / 0.18 ms | about 2,900 -> 800 | faster / fewer |
| `ui.frame.mail.large` | 2.10 ms | 0.23 ms | 0.23 ms | 2,327 -> 2,327 | ok / ok |
| `vault.open.large` | 154.94 ms | 144.93 ms | 72.77 ms | 931,321 -> 857,572 | faster / fewer |
| `vault.rescan.large` | 512.86 ms | 496.77 ms | 8.34 ms | 179,432 -> 100,634 | faster / fewer |
| `startup.app.large` | 1.25 s | 580.09 ms | 131.05 ms | 7,525,894 -> 1,639,701 | faster / fewer |
| `worst.startup` | 2.55 s | 1.32 s | 752.32 ms | 9,908,881 -> 4,963,539 | faster / fewer |
| `worst.frame.idle` | 136.28 ms | 34.55 ms | **1.55 ms** | 220,420 -> 3,919 | faster / fewer |
| `worst.frame.typing` | 163.29 ms | 127.38 ms | 30.78 ms | 2,852,985 -> 150,425 | faster / fewer |
| `worst.views.cycle` | 3.67 s | 2.56 s | 140.59 ms | 9,695,705 -> 378,064 | faster / fewer |
| `worst.memory.growth` (1000 frames, twice) | 561.08 s | 139.88 s | 27.63 s | 1,059 M -> 111 M | faster / fewer |

**Bold** = inside the 16.7 ms budget of a frame at 60 Hz. Still over it: `ui.frame.sidebar.large`
(17.4 ms, every folder open), `ui.type.editor.long` and `worst.frame.typing` (30 ms: a keystroke
in a 200 KB note, #375), `ui.frame.graph.large` (25 ms, bounded by the 6 ms budget of the physics
plus the drawing of 5000 nodes). Overall verdict of the `full` comparison: `regression`
(exit code 1) by three scenarios (below), 35 scenarios faster, 9 noisy, 1 changed; `worst`:
`faster` (exit 0), all five.

What the comparison flags, and what it is:

- `ui.frame.graph.small` (+42 % time, +123 % allocations; 1.7 -> 2.4 ms) and `graph.layout.small`
  (+50 % time and +51 % allocations: 10 steps of 200 notes, 4.1 -> 6.2 ms): a real regression of
  #382: the new force routine (`graph_tree::forces`) allocates per step even below 300 notes,
  where the old pair loop did not. Small in absolute terms, a few hundred microseconds a frame.
- `index.unlinked.large` +79 % peak bytes (112 -> 200 KiB): the per-note cache (#377) keeps its
  result; the time is 30 % lower and allocations 38 % fewer (inside the spread of the runs, so the
  time says `noisy`). Accepted.
- `ui.frame.graph.large` / `graph.layout.large`: more allocations (the quadtree) for 9 to 5 times less
  time: `tradeoff`, not a regression. `worst.startup`: 5.8 % more peak for half the time.
- `calendar.occurrences.year.large` is `changed` (version 2, see above) and has no verdict.
- Noisy (moved over 10 % inside the spread, shared computer): `index.backlinks.large`,
  `math.typeset.formulas`, `table.parse.long`, `social.index.add.large`, `pdf.export.note.long`
  (the last two are I/O and compression bound).

A first comparison of `c4918fe` itself (before the harness waited for quiet threads) said 19
regressions in scenarios nobody touched (`syntax.highlight.long`, `table.parse.long`, ...): a
background job left behind by a fast graph frame was counted into the following scenarios'
allocations. See "Quiet threads" above; this is why Y is `4858dc5` and not `c4918fe`.

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
| 1 | **The status bar counts the open tasks every frame**: `all_tasks()` clones every task of every note and sorts them with a comparator that builds two lower-cased path strings per comparison | `src/layout.rs:951` calls `Vault::all_tasks` `src/vault.rs:711` (comparator `:713`) | 37 to 60 % of every window frame at 5000 notes: idle 53 %, editor 45 %, typing 39 %, sidebar 52 %, drawing 53 %, the first frame of start-up 61 %. `index.tasks.large` is the same call: 75.5 ms, 2.6 M allocations | Count the open tasks once per `vault.generation` (PERF-001); never sort to count; where a sorted list is needed sort with `sort_by_cached_key` or by an index, not by allocating in the comparator (the same comparator is in `src/vault.rs:393`, `:649` and `:690`) Status: **Fixed** in phase 2 (`vault_cache.rs`, sorts with cached keys; #377). |
| 2 | **Backlinks and unlinked mentions of the open note are computed every frame** (right panel and status bar), each reading the text of every note | `src/panels.rs:514` `Vault::backlinks` `src/vault.rs:632`, `src/panels.rs:515` `Vault::unlinked_mentions` `:654`, `src/layout.rs:932` (`backlinks().len()` for the status bar) | `unlinked_mentions` 16 to 19 % of an idle frame, a note with few links; backlinks 26.8 ms and unlinked 27.6 ms per call in `index.backlinks.large`/`index.unlinked.large` | Compute per (note, `vault.generation`) and keep the result; unlinked mentions could be searched lazily when the panel section is opened Status: **Fixed** in phase 2: per note and vault state in `VaultCache`, unlinked mentions skip notes without the name (#377). |
| 3 | **Looking up a file by path is a linear search over `PathBuf == &Path`**, which compares component by component, and it is called per row | `Vault::file` `src/vault.rs:547` (called from `src/panels.rs:625` `mention_block`, `:416`, `:687`, `:717`) | a note with many backlinks (`ui.frame.backlinks.large`, 470.6 ms a frame): 56 % in `mention_block`, 48 % of it `PathBuf::eq`. The task view `ui.frame.tasks.large` (566.5 ms) pays the same per task group: 48 % in `Components::next_back` | A `HashMap<PathBuf, usize>` of the files built with `rebuild_name_index` (`src/vault.rs:460`) Status: **Fixed** in phase 2: `Vault::file` is a hash lookup (#378). |
| 4 | **The 3-second rescan runs on the window's thread and has a quadratic check** | `src/app.rs:2726` calls `ShaleApp::rescan` `:1352`, `Vault::rescan` `src/vault.rs:381`; the `any(\|o\| o.rel == f.rel)` check at `:401` is the quadratic part, the sort at `:393` allocates per comparison | `vault.rescan.large` 512.9 ms (15 to 20 % of the time of every frame scenario, a stall of that length every 3 s; 95 % of the scan is path comparison) | A `HashSet` of the old paths (PERF-002), `sort_by_cached_key`, and the scan as a background job (JOB-001) that hands over its result Status: **Fixed on the window's thread** (40 ms instead of 513 ms, no quadratic check, cached sort keys; #378). Not moved to a background job: 40 ms every 3 s does not need one. |
| 5 | **The editor rebuilds the layout job of the whole note every frame**, though the text did not change | `src/editor.rs:1712` (the `layouter` closure: `highlight_with_math` `:268`, `make_room_for_numbers` `:494`) | `ui.frame.editor.long` 189.9 ms and `ui.type.editor.long` 161.3 ms: the editor is 14 to 25 % of the frame, 9 to 15 % in the layouter itself; the galley (egui's text layout, `editor.layout.long` 74.7 ms) is what a keystroke must pay, but a frame without an edit should pay nothing | Keep the `LayoutJob`/galley by (text hash, width, fonts, preview range) and rebuild only when one changes; lay out only the visible part of a 200 KB note Status: **Covered by #372** (the editor keeps the laid-out note): `ui.frame.editor.long` 190 ms -> 2.8 ms. A keystroke in a very large note is #375, open. |
| 6 | **The graph's physics is quadratic and runs three steps every frame** | `GraphView::step` `src/graph.rs:124`, called three times per frame at `:193` | `ui.frame.graph.large` 312.9 ms (63 % in `step`); `graph.layout.large` 778.2 ms for 10 steps (97 %); PERF-003 already says "a few hundred notes" | A grid or Barnes-Hut repulsion, steps bounded by a time budget, stop when the energy is low (`:193` keeps going while it is above 0.05), a smaller sample for the local graph Status: **Fixed** in phase 2: quadtree repulsion above 300 notes, a 6 ms budget for the steps (#382). |
| 7 | **Opening a vault parses every note on one thread, and the window waits for it** | `Vault::open` `src/vault.rs:356` → `rescan` → `reparse` `:520`: `markdown::find_links` `src/markdown.rs:161` (31 %), `find_tags` `:363` (28 %), `find_tasks` `:254` (11 %), `inline_code_ranges` `:104` (10 %), `Vault::resolve` (6 %) | `vault.open.large` 154.9 ms (931 k allocations, 47.9 MiB); `startup.app.large` 1.25 s (its first frame is 61 % fix 1; 5 frames are in it) | Parse on worker threads (the notes are independent), open the window first and index in the background (JOB-001), scan each line once for links, tags, tasks and code spans instead of four times Status: **Partly fixed**: the notes are parsed on up to 8 threads (#384); the walk and the reading of the files are still on one thread, and the window still waits for the index. |
| 8 | **Search over the vault is a lower-casing scan of every line, and it runs again every frame while a query is typed** | `Vault::search` `src/vault.rs:731` (`markdown::find_ci` `src/markdown.rs:991`: 80 %), called per frame at `src/panels.rs:339`; `search_all::wildcard_find` `src/search_all.rs:66` builds a `Vec<(usize, char)>` per line (41 %) | `search.sidebar.large` 89.4 ms; `search.all.large` (a word no note has) 101.5 ms; with a wildcard 195.3 ms and 1.3 M allocations; a word most notes have stops at the limit of hits: 87.7 µs | Search on the key press (not every frame), on a worker thread; lower-case each note once when it is indexed; no allocation per line Status: **Partly fixed**: the sidebar search is kept per query and vault state, ASCII lines are searched without being taken apart, `wildcard_find` keeps its buffers (#379). Not on a worker thread. |
| 9 | **The task view builds and lays out every task every frame** | `src/panels.rs:644` (`all_tasks()` again, 1), per-row `v.file` (3) | `ui.frame.tasks.large` 566.5 ms, 6.0 M allocations | 1 and 3, then `ScrollArea::show_rows` (only the visible rows) Status: **Fixed** in phase 2: the cached task list and only the visible rows (#377). |
| 10 | **The calendar view recomputes the occurrences every frame** | `CalendarStore::occurrences` `src/calendar/store.rs:450` sorts `Occ` (50 % in the sort) over `ics::occurrences` `src/calendar/ics.rs:613` (`localtime_r` per occurrence) | `calendar.occurrences.year.large` 59.6 ms; `ui.frame.calendar.large` 17.3 ms a frame | Cache per (`generation`, range); `sort_by_cached_key`; one time-zone lookup per event, not per occurrence Status: **Fixed** in phase 2: the occurrences of a range are kept until the events change (#380). |
| 11 | **Opening the mail store checks the whole index every time** | `db::quick_check` `src/mail/db.rs:180` (SQLite's `quick_check` with the FTS integrity check), called from `prepare_index` `src/mail/store.rs:660` (`MailStore::open` `:211`), and `reconcile` `src/mail/store.rs:703` | `mail.open.large` 239.0 ms for 20,000 messages: 73 % `quick_check`, 26 % `reconcile`; grows with the inbox | Check only after an unclean shutdown or on request, `reconcile` from the directory's change time; it is on a background job already, so the window does not freeze, but the inbox is empty for that long Status: **Partly fixed**: `reconcile` asks for the known messages once per folder (#385); `quick_check` stays (it is MAIL-006's integrity check), the owner decides. |
| 12 | **Feed view: thousands of posts are laid out each frame** | `ShaleApp::social_view` `src/app/social.rs:16` (91 % of the frame) | `ui.frame.social.large` 39.7 ms for 3000 posts (336 k allocations); `social.index.add.large` 111.9 ms for 3000 posts (the FTS writes: `src/social/post_index.rs:115`) | Virtualise the list (`show_rows` with measured heights); batch the index writes off the window's thread Status: **Fixed** in phase 2: posts far from the view keep their measured space, the index writes with prepared statements (#381). |
| 13 | **Memory that stays**: typing keeps about 86 KB per key in a 200 KB note; a round through the views keeps tens of MB the first times | typing: the undo history (suspect: `History::checkpoint` `src/undo.rs:119`, which keeps a copy of the note and the steps of the edits); views: not found yet | `ui.type.editor.long` kept 2.5 MiB in 30 keys; `worst.frame.typing` 2.5 MiB; `worst.views.cycle` kept 118.3 MiB in 3 rounds, yet `worst.memory.growth` 510.9 KiB in 1000 frames with 10 rounds: it is a first-visit cost, not a leak, but 40 MB per round is worth a look (suspects: the graph, the mail list, the feed) | Store undo steps as diffs; find what the first visit of a view keeps with `retained_bytes` per view (add scenarios) Status: **No unbounded growth found** (#383): typing keeps nothing that grows; the 118 MiB of `worst.views.cycle` is the first visit of the graph (60 MiB), the feed (174 MiB, freed again) and the long note's galley (44 MiB), all bounded. |
| 14 | **PDF export spends a third of its time compressing** | `deflate` `src/pdf/write.rs:443` (`compress_to_vec_zlib(…, 6)`), `Page::text` | `pdf.export.note.long` 113.1 ms (38 % `miniz_oxide`), `pdf.export.book.docx` 1.02 s (9.6 M allocations, 79.7 MiB) | Level 1 to 3 for page streams, compress on worker threads; fewer allocations per drawn line Status: **Partly fixed**: the bundled fonts are compressed once per run (#386); level 6 stays, because level 3 saves 10 % of the time for 3 to 5 % more bytes and level 1 saves 18 % for 24 to 40 % more. |

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

**Phase 2 (commits after the merge of `origin/main`, 2026-11-19):** the fixes above are in. The benchmark
scenario `calendar.occurrences.year.large` now shifts its year by a day each iteration, because the
store keeps the ranges it made and the old scenario would have measured the cache; it is version 2
now, so a comparison across the change says `changed`.

**Status after phase 2** (see "Before/after"): weak spots 1, 2, 3, 4, 5 (editor cache, #372), 6, 9, 10
and 12 are fixed; 7 (the vault open waits for the index), 8 (search not on a worker thread), 11
(`quick_check`, the owner decides) and 14 (PDF compression level) are partly fixed; 13 found no
growth. Open: a keystroke in a very large note (30 ms, #375), the file list with every folder open
(17 ms), and the small-graph regression of #382 (issue below).

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
