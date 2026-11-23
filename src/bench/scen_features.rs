//! Scenarios of the features that are not the notes themselves (BUILD-028): the vault check,
//! recovery and start-up, code highlighting, formulas, tables, vim, drawings, audio, the video recorder.

use super::content_gen;
use super::scenarios::{frame_body, Body, Fixtures, Prep, Rig, Scenario};
use super::vault_gen::Size;
use std::hint::black_box;
use std::path::{Path, PathBuf};

fn vault_check_large(fx: &mut Fixtures) -> Body {
    let g = fx.vault(Size::Large);
    Box::new(move || {
        black_box(crate::vault_check::check(&g.root, &crate::busy::Cancel::default()).expect("the vault is read"));
    })
}

/// A folder of names that break on other systems: reserved words, forbidden characters, a trailing dot.
fn vault_check_problems(fx: &mut Fixtures) -> Body {
    let dir = fx.scratch("check-problems");
    for i in 0..600 {
        let name = match i % 3 {
            0 => format!("what?{i}.md"),
            1 => format!("a:b {i}.md"),
            _ => format!("trailing{i}.md."),
        };
        std::fs::write(dir.join(name), "x").expect("a file with an unportable name");
    }
    std::fs::write(dir.join("con.md"), "x").expect("a reserved name");
    Box::new(move || {
        black_box(crate::vault_check::check(&dir, &crate::busy::Cancel::default()).expect("the folder is read"));
    })
}

fn recovery_note(i: usize) -> String {
    format!("# Recovered {i}\n\n{}\n", "text of a note that was not saved. ".repeat(60))
}

fn recovery_rescue(fx: &mut Fixtures) -> Body {
    let (base, n) = (fx.scratch("recovery-rescue"), fx.scale.recovery_notes);
    let mut run = 0;
    Box::new(move || {
        run += 1;
        let root = base.join(format!("run{run}"));
        for i in 0..n {
            let day = format!("2026-11-{:02}", 1 + i % 28);
            black_box(crate::recovery::rescue(&root, &day, Path::new(&format!("Area {}/Note {i}.md", i % 7)), recovery_note(i).as_bytes()).expect("a rescued note is written"));
        }
    })
}

/// A recovery folder as a person who crashed a few times has it: a folder for each day, some seen.
fn fill_recovery(root: &Path, notes: usize) {
    for i in 0..notes {
        let day = format!("2026-{:02}-{:02}", 1 + i % 11, 1 + i % 28);
        crate::recovery::rescue(root, &day, Path::new(&format!("Area {}/Note {i}.md", i % 7)), recovery_note(i).as_bytes()).expect("a rescued note is written");
    }
    let dirs = crate::recovery::unseen(root);
    crate::recovery::dismiss(&dirs[..dirs.len() / 2]);
}

fn recovery_unseen(fx: &mut Fixtures) -> Body {
    let root = fx.scratch("recovery-unseen");
    fill_recovery(&root, fx.scale.recovery_notes.max(20) * 3);
    Box::new(move || {
        let u = crate::recovery::unseen(&root);
        black_box(u.iter().filter(|d| crate::recovery::is_inside(&root, d)).count());
    })
}

/// Removes what a start-up scenario put in the temporary folder.
pub struct Cleanup(Vec<PathBuf>);
impl Drop for Cleanup {
    fn drop(&mut self) {
        for p in &self.0 {
            let _ = std::fs::remove_dir_all(p);
        }
    }
}

/// The window of a vault with leftovers of a crash: recovered notes waiting, temporary files of
/// interrupted writes and a stale undo history. Where the app looks for them is fixed by its process.
pub fn crash_leftovers(vault: &Path, notes: usize) -> Cleanup {
    let recovery = std::env::temp_dir().join(format!("shale-recovery-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&recovery);
    fill_recovery(&recovery, notes);
    for i in 0..notes.min(200) {
        // what an interrupted `write_atomic` leaves behind: hidden, so the scan must skip it
        let _ = std::fs::write(vault.join(format!(".shale-tmp-{i}-1-2")), "half written");
    }
    Cleanup(vec![recovery])
}

fn startup_build_large(fx: &mut Fixtures) -> Body {
    let g = fx.vault(Size::Large);
    Box::new(move || {
        let mut rig = Rig::new(&g);
        rig.fr.settle(&mut rig.app, 3);
        black_box(&rig.app.vault);
    })
}

fn startup_recovery_large(fx: &mut Fixtures) -> Body {
    let g = fx.vault(Size::Large);
    let _left = crash_leftovers(&g.root, fx.scale.recovery_notes);
    Box::new(move || {
        let _keep = &_left;
        let mut rig = Rig::new(&g);
        rig.fr.settle(&mut rig.app, 3);
        black_box(rig.app.recovery.notice.len());
    })
}

fn syntax_highlight_long(fx: &mut Fixtures) -> Body {
    let text = fx.long_text(Size::Large);
    let mut n = 0;
    Box::new(move || {
        // a block the cache has not seen: the cache is by the text of the code
        n += 1;
        for b in crate::syntax::fenced_blocks(&text) {
            let code = format!("{}\n// run {n}", &text[b.body.clone()]);
            black_box(crate::syntax::highlight(&b.lang, &code));
        }
    })
}

/// A TypeScript block of 200 lines that is new for every `n` (the cache of highlighted blocks is by text).
fn ts_block(n: u32) -> String {
    (0..200).map(|i| format!("export const v{n}_{i}: Map<string, number> = new Map([[\"k{i}\", {i} * 2]]); // note {i}\n")).collect()
}

/// What the UI thread paid for a 200-line TypeScript block before #245 (EDIT-121), and what the exports still pay.
fn syntax_highlight_ts200(_fx: &mut Fixtures) -> Body {
    let mut n = 0;
    Box::new(move || {
        n += 1;
        black_box(crate::syntax::highlight("ts", &ts_block(n)));
    })
}

/// What the UI thread pays now (EDIT-121): the look-up and the queueing of the same block, which the worker colours.
fn syntax_shown_ts200(_fx: &mut Fixtures) -> Body {
    let mut n = 1_000_000;
    Box::new(move || {
        n += 1;
        black_box(crate::syntax::highlight_shown("ts", &ts_block(n)));
    })
}

const FORMULAS: &[&str] = &[
    r"\frac{a+b}{c-d}",
    r"\sqrt{x^2+y^2}",
    r"\sum_{i=1}^{n} \frac{1}{i^2} = \frac{\pi^2}{6}",
    r"\int_0^\infty e^{-x^2}\,dx = \frac{\sqrt{\pi}}{2}",
    r"\begin{pmatrix} a & b \\ c & d \end{pmatrix}",
    r"\alpha + \beta \cdot \gamma \leq \delta",
    r"\lim_{x\to 0} \frac{\sin x}{x} = 1",
    r"f(x) = \left( \frac{x}{2} \right)^{3}",
];

fn math_typeset(_fx: &mut Fixtures) -> Body {
    let mut n = 0u64;
    Box::new(move || {
        // a term that is new every time: typeset keeps the formulas it has seen
        for i in 0..200 {
            n += 1;
            black_box(crate::math::typeset(&format!("{} + x_{{{n}}}", FORMULAS[i % FORMULAS.len()]), 16.0, i % 5 == 0).is_ok());
        }
    })
}

fn table_parse_long(fx: &mut Fixtures) -> Body {
    let text = fx.long_text(Size::Large);
    Box::new(move || {
        let tables = crate::table::find_tables(&text);
        for t in &tables {
            let range = crate::table::byte_range(&text, &t.table_lines());
            for line in text[range].lines() {
                black_box(crate::table::split_cells(line));
            }
        }
        black_box(tables.len());
    })
}

fn vim_keys_long(fx: &mut Fixtures) -> Body {
    let mut text = fx.long_text(Size::Large);
    let (mut vim, mut hist, mut fx_) = (crate::vim::Vim::default(), crate::undo::History::default(), crate::vim::Effects::default());
    Box::new(move || {
        for k in "gg5jwwwbbb$0jjjxu".chars().chain("kkk3wdwu".chars()) {
            vim.key(crate::vim::VimKey::Char(k), &mut text, &mut hist, &mut fx_);
        }
        black_box(text.len());
    })
}

fn drawing_scene_large(fx: &mut Fixtures) -> Body {
    let json = content_gen::drawing(9, fx.scale.drawing);
    Box::new(move || {
        let scene = crate::drawing::Scene::parse(&json).expect("the drawing parses");
        black_box(scene.bounds());
        black_box(scene.to_json());
    })
}

fn ui_drawing_large(fx: &mut Fixtures) -> Body {
    let g = fx.vault(Size::Large);
    frame_body(Rig::new(&g).open_note(&g.drawing))
}

/// A small vault with one extra note, open in the window.
fn rig_with_note(fx: &mut Fixtures, name: &str, text: &str) -> Rig {
    let g = fx.vault(Size::Small);
    let dir = fx.scratch("special");
    std::fs::copy(g.root.join("Long note 1.md"), dir.join("Long note 1.md")).expect("a copy of the long note");
    std::fs::write(dir.join(name), text).expect("the special note");
    Rig::at(&dir).open_note(Path::new(name))
}

fn ui_math_note(fx: &mut Fixtures) -> Body {
    let text: String = (0..150).map(|i| format!("Formula {i}: ${}$ and more text around it.\n\n$$\n{}\n$$\n\n", FORMULAS[i % FORMULAS.len()], FORMULAS[(i + 3) % FORMULAS.len()])).collect();
    frame_body(rig_with_note(fx, "Math.md", &text))
}

fn ui_tables_note(fx: &mut Fixtures) -> Body {
    let table = "| Item | Count | Note |\n|---|---:|---|\n| apple | 12 | red |\n| pear | 7 | green |\n| plum | 31 | **dark** |\n\n";
    let text: String = (0..60).map(|i| format!("## Table {i}\n\n{table}")).collect();
    frame_body(rig_with_note(fx, "Tables.md", &text))
}

fn ui_code_note(fx: &mut Fixtures) -> Body {
    let code = "```rust\nfn total(items: &[u32]) -> u32 {\n    items.iter().sum()\n}\n```\n\n```python\ndef fib(n):\n    return n if n < 2 else fib(n - 1) + fib(n - 2)\n```\n\n";
    let text: String = (0..60).map(|i| format!("## Code {i}\n\n{code}")).collect();
    frame_body(rig_with_note(fx, "Code.md", &text))
}

fn ui_reading_long(fx: &mut Fixtures) -> Body {
    let g = fx.vault(Size::Large);
    let mut rig = Rig::new(&g).open_note(&g.long_note);
    let tab = rig.app.tabbar.active;
    rig.app.tabbar.tabs[tab].reading = true;
    rig.fr.settle(&mut rig.app, 4);
    frame_body(rig)
}

/// The quick switcher: every name of the vault scored against what was typed so far.
fn picker_fuzzy_large(fx: &mut Fixtures) -> Body {
    let v = super::scenarios::open(&fx.vault(Size::Large));
    let names: Vec<String> = v.files.iter().map(|f| f.display_name()).collect();
    Box::new(move || {
        let mut found = 0;
        for q in ["n", "note 42", "note 42 gar"] {
            found += names.iter().filter(|n| crate::markdown::fuzzy_score(q, n).is_some()).count();
        }
        black_box(found);
    })
}

fn ui_vim_long(fx: &mut Fixtures) -> Body {
    let g = fx.vault(Size::Large);
    let mut rig = Rig::new(&g).open_note(&g.long_note);
    rig.app.cfg.vim_mode = true;
    rig.cursor_in_the_middle(&g.long_note);
    let mut i = 0;
    Box::new(move || {
        i += 1;
        let key = ["j", "w", "k", "b"][i % 4];
        rig.frame(vec![eframe::egui::Event::Text(key.into())]);
    })
}

fn media_audio_decode(fx: &mut Fixtures) -> Prep {
    let dir = fx.scratch("audio");
    let file = dir.join("tone.wav");
    std::fs::write(&file, content_gen::wav(if fx.scale.docx_kb > 500 { 60 } else { 10 })).map_err(|e| e.to_string())?;
    Ok(Box::new(move || {
        let mut src = crate::media::decode::open(&file).expect("the tone opens");
        let mut conv = crate::media::decode::Convert::new(48_000, 2);
        let (mut buf, mut out) = (Vec::new(), Vec::new());
        while let Ok(Some((rate, ch))) = src.next(&mut buf) {
            out.clear();
            conv.process(&buf, rate, ch, &mut out).expect("resampling works");
            black_box(out.len());
        }
    }))
}

fn recorder_encode(fx: &mut Fixtures) -> Prep {
    crate::recorder::probe(crate::recorder::FFMPEG).map_err(|e| format!("skipped: {e}"))?;
    let dir = fx.scratch("recorder");
    let size = [640usize, 360];
    let frame: Vec<u8> = (0..size[0] * size[1] * 4).map(|i| (i * 7 % 251) as u8).collect();
    let mut n = 0;
    Ok(Box::new(move || {
        n += 1;
        let mut rec = crate::recorder::Recorder::start(crate::recorder::FFMPEG, &dir.join(format!("clip{n}.webm")), size).expect("ffmpeg starts");
        for _ in 0..30 {
            rec.push(size, &frame).expect("a frame is taken");
        }
        black_box(rec.finish().expect("the clip is written"));
    }))
}

pub fn list() -> Vec<Scenario> {
    let p = Scenario::plain;
    vec![
        p("vault.check.large", 10, true, vault_check_large),
        p("vault.check.problems", 20, false, vault_check_problems),
        p("startup.recovery.rescue", 5, false, recovery_rescue),
        p("startup.recovery.unseen", 20, false, recovery_unseen),
        p("startup.app.large", 3, true, startup_build_large).cold(),
        p("startup.app.recovery.large", 3, false, startup_recovery_large).cold(),
        p("syntax.highlight.long", 5, false, syntax_highlight_long),
        p("syntax.highlight.typescript", 10, false, syntax_highlight_ts200),
        p("syntax.shown.typescript", 10, false, syntax_shown_ts200),
        p("math.typeset.formulas", 10, false, math_typeset),
        p("table.parse.long", 20, false, table_parse_long),
        p("vim.keys.long", 20, false, vim_keys_long),
        p("drawing.scene.large", 10, false, drawing_scene_large),
        p("ui.frame.drawing.large", 20, false, ui_drawing_large),
        p("ui.frame.math.note", 20, false, ui_math_note),
        p("ui.frame.tables.note", 20, false, ui_tables_note),
        p("ui.frame.code.note", 20, false, ui_code_note),
        p("ui.type.vim.long", 20, false, ui_vim_long),
        p("ui.frame.reading.long", 20, false, ui_reading_long),
        p("picker.fuzzy.large", 20, false, picker_fuzzy_large),
        Scenario::maybe("media.audio.decode", 5, false, media_audio_decode),
        Scenario::maybe("recorder.encode", 3, false, recorder_encode),
    ]
}
