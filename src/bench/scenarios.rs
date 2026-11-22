//! The scenarios (BUILD-028): each has a stable ID, builds its own state (not measured) and
//! returns the closure that one iteration runs. The IDs are the keys of a result file, so a
//! renamed scenario is a different scenario; add new ones, do not reuse an ID for other work.

use super::frames::Frames;
use super::runner::{self, Report, ScenarioResult};
use super::vault_gen::{self, Generated, Size, Sizes};
use crate::app::{Options, RightPanel, ShaleApp, View};
use crate::vault::Vault;
use eframe::egui::{self, text::CCursor, text::CCursorRange, Event, Modifiers, Pos2};
use std::hint::black_box;
use std::path::{Path, PathBuf};

pub type Body = Box<dyn FnMut()>;
/// What a scenario's setup gives: the body of an iteration, or the reason it cannot run here.
pub type Prep = Result<Body, String>;

/// Which scenarios a run takes and how big its data is (BUILD-031).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Profile {
    /// small data and few iterations: a minute or so, for landing and development
    Quick,
    /// every scenario at the standard sizes: for a release
    Full,
    /// the `worst.*` scenarios: a user who uses everything at once
    Worst,
}

impl Profile {
    pub fn name(self) -> &'static str {
        match self {
            Profile::Quick => "quick",
            Profile::Full => "full",
            Profile::Worst => "worst",
        }
    }

    pub fn parse(s: &str) -> Option<Profile> {
        [Profile::Quick, Profile::Full, Profile::Worst].into_iter().find(|p| p.name() == s)
    }
}

/// How much data the scenarios make.
#[derive(Clone, Copy, Debug)]
pub struct Scale {
    pub vault: Sizes,
    /// messages in the big inbox
    pub mail: usize,
    /// calendar events
    pub events: usize,
    /// social posts
    pub posts: usize,
    /// Word files and their size (KB of text)
    pub docx_files: usize,
    pub docx_kb: usize,
    /// media files for git-LFS and their size (KB)
    pub lfs_files: usize,
    pub lfs_kb: usize,
    /// elements of the big drawing
    pub drawing: usize,
    /// frames of the memory growth run
    pub leak_frames: usize,
    /// notes in the recovery folder
    pub recovery_notes: usize,
    /// at most this many iterations of any scenario
    pub max_iters: usize,
}

impl Scale {
    pub const FULL: Scale =
        Scale { vault: Sizes::STANDARD, mail: 20_000, events: 5_000, posts: 3_000, docx_files: 3, docx_kb: 1_500, lfs_files: 6, lfs_kb: 4_096, drawing: 2_000, leak_frames: 1_000, recovery_notes: 300, max_iters: usize::MAX };
    pub const QUICK: Scale = Scale { vault: Sizes { small: 60, large: 600 }, mail: 1_500, events: 400, posts: 300, docx_files: 1, docx_kb: 200, lfs_files: 2, lfs_kb: 512, drawing: 300, leak_frames: 200, recovery_notes: 40, max_iters: 5 };
    /// for the tests of the harness itself
    #[cfg(test)]
    pub const TINY: Scale = Scale { vault: Sizes { small: 12, large: 30 }, mail: 60, events: 30, posts: 20, docx_files: 1, docx_kb: 20, lfs_files: 1, lfs_kb: 64, drawing: 30, leak_frames: 8, recovery_notes: 5, max_iters: 2 };

    pub fn of(p: Profile) -> Scale {
        if p == Profile::Quick {
            Scale::QUICK
        } else {
            Scale::FULL
        }
    }
}

enum Prepare {
    Plain(fn(&mut Fixtures) -> Body),
    Maybe(fn(&mut Fixtures) -> Prep),
}

pub struct Scenario {
    pub id: &'static str,
    /// iterations when `--iters` is not given: fewer for the slow ones
    pub iters: usize,
    /// part of the `quick` profile (the `full` profile has every scenario but the `worst.*` ones)
    pub quick: bool,
    /// too costly to run twice: no warm-up run
    cold: bool,
    /// bumped when a change makes the scenario measure something else (BUILD-036)
    pub version: u32,
    prepare: Prepare,
}

impl Scenario {
    pub fn plain(id: &'static str, iters: usize, quick: bool, f: fn(&mut Fixtures) -> Body) -> Scenario {
        Scenario { id, iters, quick, cold: false, version: 1, prepare: Prepare::Plain(f) }
    }

    /// A scenario that may be skipped (a program is missing): its setup says why.
    pub fn maybe(id: &'static str, iters: usize, quick: bool, f: fn(&mut Fixtures) -> Prep) -> Scenario {
        Scenario { id, iters, quick, cold: false, version: 1, prepare: Prepare::Maybe(f) }
    }

    /// This scenario's definition changed: its results of an older version are not comparable (BUILD-036).
    pub fn version(mut self, version: u32) -> Scenario {
        self.version = version;
        self
    }

    /// Run once, without a warm-up, `iters` times.
    pub fn cold(mut self) -> Scenario {
        self.cold = true;
        self
    }

    fn worst(&self) -> bool {
        self.id.starts_with("worst.")
    }
}

/// The generated data, made when the first scenario that needs it asks, in a temporary folder
/// that is removed at the end.
pub struct Fixtures {
    base: PathBuf,
    pub scale: Scale,
    small: Option<Generated>,
    large: Option<Generated>,
    cache: std::collections::HashMap<&'static str, PathBuf>,
    counter: usize,
    /// the files of the worst-case window, made when the first `worst.*` scenario asks
    pub world: Option<super::worst::WorldData>,
    /// what could not be made for the scenarios that ran on it
    pub skipped: Vec<runner::Skipped>,
}

impl Fixtures {
    pub fn new(scale: Scale) -> Fixtures {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        Fixtures::in_folder(scale, std::env::temp_dir().join(format!("shale-bench-{}-{}", std::process::id(), NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed))))
    }

    /// Fixtures made below `base` (which may have a space in its path, like a temporary folder can).
    pub(super) fn in_folder(scale: Scale, base: PathBuf) -> Fixtures {
        let _ = std::fs::remove_dir_all(&base);
        Fixtures { base, scale, small: None, large: None, cache: Default::default(), counter: 0, world: None, skipped: vec![] }
    }

    pub fn vault(&mut self, size: Size) -> Generated {
        let (slot, name, long_notes, drawing) = match size {
            Size::Small => (&mut self.small, "small", 1, self.scale.drawing / 10),
            Size::Large => (&mut self.large, "large", 5, self.scale.drawing),
        };
        if slot.is_none() {
            let root = self.base.join(name);
            std::fs::create_dir_all(&root).expect("a temporary folder for the benchmark vault");
            *slot = Some(vault_gen::generate(&root, self.scale.vault.notes(size), long_notes, drawing).expect("the benchmark vault is written"));
        }
        slot.clone().expect("just made")
    }

    /// The text of the long note of a vault.
    pub fn long_text(&mut self, size: Size) -> String {
        let g = self.vault(size);
        std::fs::read_to_string(g.root.join(&g.long_note)).expect("the long note is there")
    }

    /// A new empty folder of its own.
    pub fn scratch(&mut self, name: &str) -> PathBuf {
        self.counter += 1;
        let d = self.base.join(format!("{name}-{}", self.counter));
        std::fs::create_dir_all(&d).expect("a temporary folder");
        d
    }

    /// A folder that `make` fills once and every scenario that asks for `key` shares (read only!).
    pub fn cached(&mut self, key: &'static str, make: impl FnOnce(&Path) -> Result<(), String>) -> Result<PathBuf, String> {
        if let Some(p) = self.cache.get(key) {
            return Ok(p.clone());
        }
        let dir = self.scratch(key);
        make(&dir)?;
        self.cache.insert(key, dir.clone());
        Ok(dir)
    }
}

impl Drop for Fixtures {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

pub fn open(g: &Generated) -> Vault {
    Vault::open(&g.root).expect("the benchmark vault opens")
}

// ---- the window ---------------------------------------------------------------------------

/// A window on a vault, settled, and its frame runner.
pub struct Rig {
    pub app: ShaleApp,
    pub fr: Frames,
}

impl Rig {
    pub fn new(g: &Generated) -> Rig {
        Rig::at(&g.root)
    }

    pub fn at(root: &Path) -> Rig {
        let app = ShaleApp::build(Options { screenshot: None, vault: Some(root.to_path_buf()), view: None, keys: None }, false);
        let mut rig = Rig { app, fr: Frames::new() };
        rig.fr.settle(&mut rig.app, 5);
        rig
    }

    pub fn open_note(mut self, rel: &Path) -> Rig {
        self.app.open(rel, false);
        self.fr.settle(&mut self.app, 6);
        let shown = &self.app.tabbar.tabs[self.app.tabbar.active].view;
        assert!(shown.path() == Some(rel), "the window shows {shown:?}, not {}", rel.display());
        self
    }

    /// A scenario that measures the wrong view measures nothing: stop when the window shows another one.
    pub fn assert_view(&self, want: &View) {
        let shown = &self.app.tabbar.tabs[self.app.tabbar.active].view;
        // the kind of view: a feed tab fills in which feed it shows
        assert!(std::mem::discriminant(shown) == std::mem::discriminant(want), "the window shows {shown:?}, not {want:?}");
    }

    /// Show a view and let the window settle.
    pub fn show(&mut self, view: View) {
        self.app.show_view(view.clone(), false);
        self.fr.settle(&mut self.app, 4);
        self.assert_view(&view);
    }

    /// Frames, a few milliseconds apart, until `ready` (background jobs report back between frames); `Err` when it takes more than 60 s.
    pub fn wait(&mut self, what: &str, ready: impl Fn(&ShaleApp) -> bool) -> Result<(), String> {
        let start = std::time::Instant::now();
        while !ready(&self.app) {
            if start.elapsed().as_secs() > 60 {
                return Err(format!("{what} did not happen in 60 s"));
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
            self.frame(vec![]);
        }
        Ok(())
    }

    /// Frames for `ms` milliseconds, so jobs that were started finish and show their result.
    pub fn run_for(&mut self, ms: u64) {
        let start = std::time::Instant::now();
        while start.elapsed().as_millis() < ms as u128 {
            std::thread::sleep(std::time::Duration::from_millis(5));
            self.frame(vec![]);
        }
    }

    pub fn frame(&mut self, events: Vec<Event>) {
        black_box(self.fr.frame(&mut self.app, events));
    }

    /// The editor of the open note has the keyboard and its cursor sits in the middle of the text.
    pub fn cursor_in_the_middle(&mut self, rel: &Path) {
        let tab = self.app.tabbar.active;
        let id = self.app.tabbar.tabs[tab].editor.id;
        self.app.tabbar.tabs[tab].editor.request_focus = true;
        self.fr.settle(&mut self.app, 3);
        let middle = self.app.notes.get(rel).map(|n| n.text.chars().count() / 2).unwrap_or(0);
        let mut st = egui::text_edit::TextEditState::load(self.fr.ctx(), id).unwrap_or_default();
        st.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(middle))));
        st.store(self.fr.ctx(), id);
        self.fr.settle(&mut self.app, 2);
    }
}

pub fn frame_body(rig: Rig) -> Body {
    let mut rig = rig;
    Box::new(move || rig.frame(vec![]))
}

pub fn wheel(dy: f32) -> Event {
    Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: egui::vec2(0.0, dy), modifiers: Modifiers::NONE, phase: egui::TouchPhase::Move, source: egui::MouseWheelSource::Wheel }
}

// ---- the scenarios ------------------------------------------------------------------------

fn vault_open(fx: &mut Fixtures, size: Size) -> Body {
    let g = fx.vault(size);
    Box::new(move || {
        black_box(open(&g));
    })
}

fn vault_rescan_large(fx: &mut Fixtures) -> Body {
    let g = fx.vault(Size::Large);
    let mut v = open(&g);
    Box::new(move || {
        black_box(v.rescan());
    })
}

fn markdown_scan_long(fx: &mut Fixtures) -> Body {
    let text = fx.long_text(Size::Large);
    Box::new(move || {
        black_box(crate::markdown::find_links(&text));
        black_box(crate::markdown::find_tasks(&text));
        black_box(crate::markdown::find_tags(&text));
        black_box(crate::markdown::find_headings(&text));
        black_box(crate::markdown::find_math(&text));
        black_box(crate::markdown::find_footnotes(&text));
        black_box(crate::markdown::fenced_code_lines(&text));
    })
}

fn links_of(text: &str) -> Vec<(crate::markdown::LinkSpan, bool)> {
    crate::markdown::find_links(text).into_iter().map(|l| (l, true)).collect()
}

fn editor_highlight_long(fx: &mut Fixtures) -> Body {
    let text = fx.long_text(Size::Large);
    let links = links_of(&text);
    Box::new(move || {
        black_box(crate::editor::highlight_preview(&text, &links, &crate::fonts::EditorFonts::uniform(15.0), &[], Some(0..0)));
    })
}

fn editor_layout_long(fx: &mut Fixtures) -> Body {
    let text = fx.long_text(Size::Large);
    let links = links_of(&text);
    let ctx = egui::Context::default();
    crate::theme::apply(&ctx);
    // one pass first: the fonts of a context exist only after it has begun a frame
    ctx.run_ui(egui::RawInput::default(), |_| {}).textures_delta.clear();
    let mut n = 0u32;
    Box::new(move || {
        // a text size that is a little different every time: egui keeps the galleys of the last frames, and a hit would measure only its hash
        n += 1;
        let fonts = crate::fonts::EditorFonts::uniform(15.0 + n as f32 * 0.001);
        let mut job = crate::editor::highlight_preview(&text, &links, &fonts, &[], Some(0..0));
        job.wrap.max_width = 700.0;
        black_box(ctx.fonts_mut(|f| f.layout_job(job)));
        // a frame goes by, as in the window: egui frees the galleys that the last frames did not use
        ctx.run_ui(egui::RawInput::default(), |_| {}).textures_delta.clear();
    })
}

fn search_sidebar_large(fx: &mut Fixtures) -> Body {
    let v = open(&fx.vault(Size::Large));
    Box::new(move || {
        black_box(crate::search_all::note_hits(&v, &crate::search_all::Query::sidebar("garden")));
    })
}

/// What the sidebar does with the hits of a note search when it first draws them (NAV-060): the job of every hit, rendered.
fn search_render_sidebar_large(fx: &mut Fixtures) -> Body {
    let v = open(&fx.vault(Size::Large));
    let hits = crate::search_all::note_hits(&v, &crate::search_all::Query::sidebar("garden"));
    let mut n = 0u32;
    Box::new(move || {
        // a size that is a little different every time: a hit keeps its job for a look, so each round makes them again
        n += 1;
        let look = crate::search_render::Look { rendered: true, size: 12.5 + n as f32 * 0.001, raw_colour: crate::theme::palette().muted };
        for h in &hits {
            black_box(h.job(look));
        }
    })
}

/// `note_hits` with a query: a word that most notes have stops at the limit of hits, one that none has reads the whole vault.
fn search_all(fx: &mut Fixtures, query: &'static str) -> Body {
    let v = open(&fx.vault(Size::Large));
    Box::new(move || {
        black_box(crate::search_all::note_hits(&v, &crate::search_all::Query::picker(query)));
    })
}

fn index_backlinks_large(fx: &mut Fixtures) -> Body {
    let g = fx.vault(Size::Large);
    let v = open(&g);
    Box::new(move || {
        black_box(v.backlinks(&g.popular));
    })
}

fn index_unlinked_large(fx: &mut Fixtures) -> Body {
    let g = fx.vault(Size::Large);
    let v = open(&g);
    Box::new(move || {
        black_box(v.unlinked_mentions(&g.popular));
    })
}

fn index_tasks_large(fx: &mut Fixtures) -> Body {
    let v = open(&fx.vault(Size::Large));
    Box::new(move || {
        black_box(v.all_tasks());
        black_box(v.all_tags());
    })
}

fn graph_build_large(fx: &mut Fixtures) -> Body {
    let v = open(&fx.vault(Size::Large));
    Box::new(move || {
        black_box(v.graph(true));
    })
}

fn graph_layout(fx: &mut Fixtures, size: Size) -> Body {
    let v = open(&fx.vault(size));
    Box::new(move || {
        let mut g = crate::graph::GraphView::new(false);
        g.rebuild(&v);
        for _ in 0..10 {
            g.step();
        }
        black_box(&g);
    })
}

fn ui_idle_large(fx: &mut Fixtures) -> Body {
    frame_body(Rig::new(&fx.vault(Size::Large)))
}

fn ui_sidebar_large(fx: &mut Fixtures) -> Body {
    let mut rig = Rig::new(&fx.vault(Size::Large));
    let folders = rig.app.vault.as_ref().map(|v| v.folders.clone()).unwrap_or_default();
    rig.app.tree.expanded.extend(folders);
    rig.fr.settle(&mut rig.app, 4);
    frame_body(rig)
}

fn ui_editor_long(fx: &mut Fixtures) -> Body {
    let g = fx.vault(Size::Large);
    frame_body(Rig::new(&g).open_note(&g.long_note))
}

fn ui_backlinks_large(fx: &mut Fixtures) -> Body {
    let g = fx.vault(Size::Large);
    let mut rig = Rig::new(&g);
    rig.app.side.layout.show_panel(RightPanel::Backlinks);
    frame_body(rig.open_note(&g.popular))
}

fn ui_type_long(fx: &mut Fixtures) -> Body {
    let g = fx.vault(Size::Large);
    let mut rig = Rig::new(&g).open_note(&g.long_note);
    rig.cursor_in_the_middle(&g.long_note);
    Box::new(move || rig.frame(vec![Event::Text("x".into())]))
}

fn ui_scroll_long(fx: &mut Fixtures) -> Body {
    let g = fx.vault(Size::Large);
    let mut rig = Rig::new(&g).open_note(&g.long_note);
    rig.frame(vec![Event::PointerMoved(Pos2::new(700.0, 450.0))]);
    Box::new(move || rig.frame(vec![wheel(-120.0)]))
}

fn ui_graph(fx: &mut Fixtures, size: Size) -> Body {
    let mut rig = Rig::new(&fx.vault(size));
    rig.show(View::Graph);
    frame_body(rig)
}

fn ui_tasks_large(fx: &mut Fixtures) -> Body {
    let mut rig = Rig::new(&fx.vault(Size::Large));
    rig.show(View::Tasks);
    frame_body(rig)
}

/// The scenarios of the core: vaults, markdown, search, the link index, the graph, the window.
fn core() -> Vec<Scenario> {
    let p = Scenario::plain;
    vec![
        p("vault.open.small", 20, true, |fx| vault_open(fx, Size::Small)),
        p("vault.open.large", 5, true, |fx| vault_open(fx, Size::Large)),
        p("vault.rescan.large", 10, false, vault_rescan_large),
        p("markdown.scan.long", 20, true, markdown_scan_long),
        p("editor.highlight.long", 20, true, editor_highlight_long),
        p("editor.layout.long", 10, false, editor_layout_long),
        p("search.sidebar.large", 10, true, search_sidebar_large),
        p("search.render.sidebar.large", 10, false, search_render_sidebar_large),
        p("search.all.large", 10, false, |fx| search_all(fx, "qqqxz")),
        p("search.all.hit.large", 10, false, |fx| search_all(fx, "garden")),
        p("search.all.wild.large", 5, false, |fx| search_all(fx, "g*qqz")),
        p("search.all.wild.hit.large", 10, false, |fx| search_all(fx, "gar*en")),
        p("index.backlinks.large", 10, true, index_backlinks_large),
        p("index.unlinked.large", 5, false, index_unlinked_large),
        p("index.tasks.large", 10, false, index_tasks_large),
        p("graph.build.large", 10, true, graph_build_large),
        p("graph.layout.small", 10, false, |fx| graph_layout(fx, Size::Small)),
        p("graph.layout.large", 3, false, |fx| graph_layout(fx, Size::Large)),
        p("ui.frame.idle.large", 60, true, ui_idle_large),
        p("ui.frame.sidebar.large", 30, false, ui_sidebar_large),
        p("ui.frame.editor.long", 30, true, ui_editor_long),
        p("ui.frame.backlinks.large", 30, false, ui_backlinks_large),
        p("ui.type.editor.long", 30, true, ui_type_long),
        p("ui.scroll.editor.long", 30, true, ui_scroll_long),
        p("ui.frame.graph.small", 30, false, |fx| ui_graph(fx, Size::Small)),
        p("ui.frame.graph.large", 5, false, |fx| ui_graph(fx, Size::Large)),
        p("ui.frame.tasks.large", 30, false, ui_tasks_large),
    ]
}

/// Every scenario, in the order they run: the core, then one group per feature of Shale.
pub fn all() -> Vec<Scenario> {
    let mut v = core();
    v.extend(super::scen_features::list());
    v.extend(super::scen_data::list());
    v.extend(super::scen_io::list());
    v.extend(super::worst::list());
    v
}

/// The IDs and scenarios a profile and filter pick. A filter is parts of IDs (`ui.frame`) or
/// `=id` for exactly one, several separated by commas (BUILD-036: running again what a comparison flagged).
fn chosen(profile: Profile, filter: Option<&str>) -> Vec<Scenario> {
    all()
        .into_iter()
        .filter(|s| match profile {
            Profile::Quick => s.quick,
            Profile::Full => !s.worst(),
            Profile::Worst => s.worst(),
        })
        .filter(|s| filter.is_none_or(|f| f.split(',').filter(|part| !part.is_empty()).any(|part| part.strip_prefix('=').map_or_else(|| s.id.contains(part), |exact| s.id == exact))))
        .collect()
}

/// Run the scenarios of `profile` whose ID contains `filter`, `iters` times each (their own
/// count without), more rounds until `min_ms` was measured; `progress` hears each result as it comes.
pub fn run(scale: Scale, profile: Profile, filter: Option<&str>, iters: Option<usize>, min_ms: Option<u64>, progress: &mut dyn FnMut(&ScenarioResult)) -> Result<Report, String> {
    let picked = chosen(profile, filter);
    if picked.is_empty() {
        return Err(format!("no scenario of the {} profile has {:?} in its ID (shale --bench-list names them)", profile.name(), filter.unwrap_or("")));
    }
    let mut fx = Fixtures::new(scale);
    let (mut results, mut skipped) = (Vec::new(), Vec::new());
    for s in picked {
        let prepared = match s.prepare {
            Prepare::Plain(f) => Ok(f(&mut fx)),
            Prepare::Maybe(f) => f(&mut fx),
        };
        match prepared {
            Ok(mut body) => {
                let n = iters.unwrap_or(s.iters).min(scale.max_iters).max(1);
                let mut r = runner::measure(s.id, n, !s.cold, min_ms, &mut *body);
                r.version = s.version;
                drop(body);
                progress(&r);
                results.push(r);
            }
            Err(reason) => {
                eprintln!("{:<34} skipped: {reason}", s.id);
                skipped.push(runner::Skipped { id: s.id.into(), reason });
            }
        }
    }
    skipped.extend(std::mem::take(&mut fx.skipped));
    Ok(runner::header(profile.name(), results, skipped))
}

/// The IDs a profile runs.
pub fn ids(profile: Profile) -> Vec<&'static str> {
    chosen(profile, None).iter().map(|s| s.id).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec: BUILD-028
    #[test]
    fn the_scenario_ids_are_unique_and_stable() {
        let all = all();
        let ids: Vec<&str> = all.iter().map(|s| s.id).collect();
        let mut sorted = ids.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len(), "no ID twice");
        for must in [
            "vault.open.small",
            "vault.open.large",
            "markdown.scan.long",
            "search.all.large",
            "index.backlinks.large",
            "graph.build.large",
            "graph.layout.large",
            "ui.frame.idle.large",
            "ui.frame.editor.long",
            "ui.type.editor.long",
            "ui.scroll.editor.long",
            "ui.frame.graph.large",
            "ui.frame.sidebar.large",
            "worst.startup",
            "worst.memory.growth",
        ] {
            assert!(ids.contains(&must), "{must}");
        }
        assert!(ids.iter().all(|id| id.chars().all(|c| c.is_ascii_lowercase() || c == '.' || c == '_')), "an ID is lower case words with dots");
    }

    // spec: BUILD-036
    #[test]
    fn a_filter_is_parts_of_ids_or_exact_ids_separated_by_commas() {
        let picked = |f: &str| chosen(Profile::Full, Some(f)).iter().map(|s| s.id).collect::<Vec<_>>();
        assert_eq!(picked("=vault.open.small,=markdown.scan.long"), ["vault.open.small", "markdown.scan.long"]);
        assert_eq!(picked("=vault.open.small,graph.layout"), ["vault.open.small", "graph.layout.small", "graph.layout.large"]);
        assert_eq!(picked("=vault.open"), Vec::<&str>::new(), "= means the whole ID");
        assert_eq!(picked("vault.open.small,"), ["vault.open.small"], "an empty part is nothing");
    }

    // spec: BUILD-031
    #[test]
    fn the_profiles_pick_their_scenarios() {
        let (quick, full, worst) = (ids(Profile::Quick), ids(Profile::Full), ids(Profile::Worst));
        assert!(!quick.is_empty() && quick.len() < full.len(), "quick is a part of full");
        assert!(quick.iter().all(|q| full.contains(q)));
        assert!(worst.iter().all(|w| w.starts_with("worst.")) && worst.len() >= 5);
        assert!(full.iter().all(|f| !f.starts_with("worst.")), "worst is a run of its own");
        assert_eq!(Profile::parse("full"), Some(Profile::Full));
        assert_eq!(Profile::parse("slow"), None);
    }

    // spec: BUILD-028, BUILD-031
    #[test]
    fn every_scenario_runs_or_says_why_it_is_skipped_on_tiny_data() {
        let _alone = runner::SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        for profile in [Profile::Full, Profile::Worst] {
            let mut seen = vec![];
            let report = run(Scale::TINY, profile, None, Some(1), None, &mut |r| seen.push(r.id.clone())).unwrap();
            assert_eq!(seen, report.scenarios.iter().map(|s| s.id.clone()).collect::<Vec<_>>());
            assert_eq!(report.scenarios.len() + report.skipped.len(), ids(profile).len(), "every scenario ran or was skipped");
            for s in &report.scenarios {
                assert_eq!(s.iters, 1, "{}", s.id);
                assert!(s.median_ns > 0 && s.min_ns <= s.median_ns && s.median_ns <= s.p95_ns, "{s:?}");
                assert!(s.allocs > 0, "{}: every scenario allocates", s.id);
            }
            for k in &report.skipped {
                assert!(!k.reason.is_empty(), "{}", k.id);
                eprintln!("skipped {}: {}", k.id, k.reason);
            }
            assert_eq!((report.schema, report.run.as_str()), (runner::SCHEMA, profile.name()));
            // spec: BUILD-036
            if let Some(s) = report.scenarios.iter().find(|s| s.id == "calendar.occurrences.year.large") {
                assert_eq!(s.version, 2, "its definition changed with the range cache of the calendar store");
            }
            assert!(report.scenarios.iter().all(|s| s.version >= 1));
            assert!(!report.commit.is_empty() && report.host.cores >= 1);
        }
    }

    // spec: BUILD-028
    #[test]
    fn a_filter_picks_scenarios_by_a_part_of_the_id_and_no_match_is_an_error() {
        let r = run(Scale::TINY, Profile::Full, Some("vault.open"), Some(1), None, &mut |_| {}).unwrap();
        assert_eq!(r.scenarios.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["vault.open.small", "vault.open.large"]);
        let e = run(Scale::TINY, Profile::Full, Some("nothing.like.this"), None, None, &mut |_| {}).unwrap_err();
        assert!(e.contains("no scenario"), "{e}");
        let exact = run(Scale::TINY, Profile::Full, Some("=vault.open.small"), Some(1), None, &mut |_| {}).unwrap();
        assert_eq!(exact.scenarios.len(), 1, "= makes the filter the whole ID");
        assert!(run(Scale::TINY, Profile::Worst, Some("vault.open"), None, None, &mut |_| {}).is_err(), "a filter stays inside the profile");
    }
}
