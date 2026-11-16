//! The `--keys` script of `--screenshot` runs (BUILD-007): steps joined by `|` that type, press
//! keys and use the mouse in the real window, so a documentation picture can be reproduced.
//!
//! Steps (a *target* is `x,y` in window points, or `@label`: the text of a label drawn in the
//! window, the exact text preferred over a part of it; `@label#2` is the second of several, in
//! the order `dump` lists them). Icon buttons draw no text: their tooltip is their label (`@Outline of
//! the note`, a part is enough), and the tabs are `@Tab 1`, `@Tab 2`, ...):
//!
//! | step | what it does |
//! |---|---|
//! | `text:abc` | types the text |
//! | `key:ctrl+shift+F` | presses a key with any modifiers (`shift`, `alt`, `ctrl`) |
//! | `move:T` `hover:T` | moves the pointer (and leaves it there) |
//! | `down:T` `up:T` | presses / releases the left button at the target |
//! | `click:T` `dblclick:T` `rclick:T` | clicks with the left / the right button |
//! | `drag:T1>T2` | presses at T1, moves in steps, releases at T2 |
//! | `scroll:N` | the mouse wheel at the pointer, N points down |
//! | `wait:N` | does nothing for N steps |
//! | `waitfor:@label` | waits until the label is drawn (the next step only then runs) |
//! | `drop:/path` | a file dropped through the window system |
//! | `drop:T:/a;;/b` | files dropped at the target by another program (Linux), so the folder, note or editor under it decides where they go |
//! | `dropdata:mime:data` | data dropped by another program at the pointer (Linux) |
//! | `dump` | prints every label drawn, with its place, to stderr (to find the `@label` of a widget) |
//! | `focus-end` | puts the editor's cursor at the end of the note |

use eframe::egui;
use std::path::PathBuf;

/// Where a pointer step acts.
#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    /// window points
    At(f32, f32),
    /// the text of a label drawn in the window, and which of several (from 1)
    Label(String, usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    Text(String),
    Key(String),
    Move(Target),
    Down(Target),
    Up(Target),
    Click(Target, Button),
    DoubleClick(Target),
    Drag(Target, Target),
    Scroll(f32),
    Wait(u32),
    WaitFor(String, usize),
    /// files dropped: through the window system (no place), or at a place the way another
    /// program drops them
    Drop(Option<Target>, Vec<PathBuf>),
    DropData(String),
    Dump,
    FocusEnd,
}

fn target(s: &str) -> Result<Target, String> {
    let s = s.trim();
    if let Some(label) = s.strip_prefix('@') {
        if label.is_empty() {
            return Err("an empty label after @".into());
        }
        if let Some((l, n)) = label.rsplit_once('#') {
            if let Ok(n) = n.parse::<usize>() {
                return if n == 0 || l.is_empty() { Err(format!("`{s}`: the number after # counts from 1")) } else { Ok(Target::Label(l.to_string(), n)) };
            }
        }
        return Ok(Target::Label(label.to_string(), 1));
    }
    let v: Vec<f32> = s.split(',').map(|n| n.trim().parse::<f32>()).collect::<Result<_, _>>().map_err(|_| format!("`{s}` is neither x,y nor @label"))?;
    match v[..] {
        [x, y] => Ok(Target::At(x, y)),
        _ => Err(format!("`{s}` is neither x,y nor @label")),
    }
}

fn step(raw: &str) -> Result<Step, String> {
    match raw {
        "focus-end" => return Ok(Step::FocusEnd),
        "dump" => return Ok(Step::Dump),
        _ => {}
    }
    let (name, arg) = raw.split_once(':').ok_or_else(|| format!("`{raw}` is not a step"))?;
    Ok(match name {
        "text" => Step::Text(arg.to_string()),
        "key" => {
            // "ctrl+shift+F": any number of modifiers before the key
            let mut k = arg;
            while let Some((m, rest)) = k.split_once('+') {
                if !matches!(m, "shift" | "alt" | "ctrl") {
                    break;
                }
                k = rest;
            }
            if eframe::egui::Key::from_name(k).is_none() {
                return Err(format!("`{k}` is not a key name"));
            }
            Step::Key(arg.to_string())
        }
        "move" | "hover" => Step::Move(target(arg)?),
        "down" => Step::Down(target(arg)?),
        "up" => Step::Up(target(arg)?),
        "click" => Step::Click(target(arg)?, Button::Left),
        "rclick" => Step::Click(target(arg)?, Button::Right),
        "dblclick" => Step::DoubleClick(target(arg)?),
        "drag" => {
            let (a, b) = arg.split_once('>').ok_or_else(|| format!("`{raw}`: drag needs `from>to`"))?;
            Step::Drag(target(a)?, target(b)?)
        }
        "scroll" => Step::Scroll(arg.trim().parse().map_err(|_| format!("`{arg}` is not a number of points"))?),
        "wait" => Step::Wait(arg.trim().parse().map_err(|_| format!("`{arg}` is not a number of steps"))?),
        "waitfor" => match target(arg)? {
            Target::Label(l, n) => Step::WaitFor(l, n),
            Target::At(..) => return Err(format!("`{raw}`: waitfor needs `@label`")),
        },
        "drop" => {
            // `drop:/a` or `drop:T:/a;;/b` (a place, then files separated by `;;`)
            let (at, files) = match arg.split_once(':').and_then(|(t, f)| target(t).ok().map(|t| (t, f))) {
                Some((t, f)) => (Some(t), f),
                None => (None, arg),
            };
            let paths: Vec<PathBuf> = files.split(";;").filter(|f| !f.is_empty()).map(PathBuf::from).collect();
            if paths.is_empty() {
                return Err(format!("`{raw}`: drop needs a file"));
            }
            Step::Drop(at, paths)
        }
        "dropdata" => Step::DropData(arg.to_string()),
        _ => return Err(format!("`{name}` is not a step")),
    })
}

/// The steps of a `--keys` argument; an empty script has no steps.
pub fn parse(script: &str) -> Result<Vec<Step>, String> {
    if script.is_empty() {
        return Ok(vec![]);
    }
    script.split('|').map(step).collect()
}

/// The point of the `nth` (from 1) text drawn (top layer first) that is exactly `label`; if no
/// text is, the same among the texts that contain it.
pub fn find_label(texts: &[(String, egui::Rect)], label: &str, nth: usize) -> Option<egui::Pos2> {
    let nth = nth.max(1) - 1;
    let exact = texts.iter().filter(|(t, _)| t.trim() == label).nth(nth);
    exact.or_else(|| texts.iter().filter(|(t, _)| t.contains(label)).nth(nth)).map(|(_, r)| r.center())
}

/// Icon buttons draw no text, so they tell the script their tooltip and place instead: the
/// tooltip is their `@label`. `None` = no script runs and nothing is kept.
static HINTS: std::sync::Mutex<Option<Vec<(String, egui::Rect)>>> = std::sync::Mutex::new(None);

/// A script runs: start keeping the hints of icon buttons.
pub fn collect_hints() {
    *HINTS.lock().unwrap_or_else(|e| e.into_inner()) = Some(Vec::new());
}

/// An icon button (or other widget without text) of this frame, found by its tooltip.
pub fn hint(tip: &str, rect: egui::Rect) {
    if let Some(v) = HINTS.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        if v.len() < 2000 {
            v.push((tip.to_string(), rect));
        }
    }
}

/// The hints of the frame so far (they are forgotten).
pub fn take_hints() -> Vec<(String, egui::Rect)> {
    HINTS.lock().unwrap_or_else(|e| e.into_inner()).as_mut().map(std::mem::take).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec: DEV-007
    #[test]
    fn the_old_steps_still_parse() {
        let s = parse("focus-end|text:[!/|key:ArrowDown|key:ctrl+shift+F|click:10,20.5|dblclick:1,2|scroll:-40|drop:/tmp/a.png").unwrap();
        assert_eq!(s.len(), 8);
        assert_eq!(s[0], Step::FocusEnd);
        assert_eq!(s[1], Step::Text("[!/".into()));
        assert_eq!(s[4], Step::Click(Target::At(10.0, 20.5), Button::Left));
        assert_eq!(s[6], Step::Scroll(-40.0));
        assert!(parse("").unwrap().is_empty());
    }

    // spec: DEV-007
    #[test]
    fn pointer_steps_take_coordinates_or_labels() {
        assert_eq!(parse("rclick:@Welcome").unwrap(), vec![Step::Click(Target::Label("Welcome".into(), 1), Button::Right)]);
        assert_eq!(parse("click:@Welcome#2").unwrap(), vec![Step::Click(Target::Label("Welcome".into(), 2), Button::Left)]);
        assert_eq!(parse("hover:5,6").unwrap(), vec![Step::Move(Target::At(5.0, 6.0))]);
        assert_eq!(parse("drag:@Calendar>900,300").unwrap(), vec![Step::Drag(Target::Label("Calendar".into(), 1), Target::At(900.0, 300.0))]);
        assert_eq!(parse("wait:3|waitfor:@Sync").unwrap(), vec![Step::Wait(3), Step::WaitFor("Sync".into(), 1)]);
    }

    // spec: DEV-007
    #[test]
    fn a_wrong_step_is_an_error_not_a_silent_skip() {
        for bad in ["click:12", "click:a,b", "click:@", "drag:1,2", "drag:@a", "key:Nonsense", "key:ctrl+", "jump:1,1", "waitfor:Sync", "waitfor:5,5", "click:@a#0", "scroll:x", "nothing"] {
            assert!(parse(bad).is_err(), "{bad} should not parse");
        }
    }

    // spec: DEV-007
    // spec: BUILD-007
    #[test]
    fn build_007_an_icon_button_is_found_by_its_tooltip_and_only_while_a_script_runs() {
        let r = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(30.0, 30.0));
        hint("Outline of the note", r);
        assert!(take_hints().is_empty(), "nothing is kept without a script");
        collect_hints();
        hint("Outline of the note", r);
        let mut texts = vec![("Outline".to_string(), egui::Rect::from_min_size(egui::pos2(100.0, 5.0), egui::vec2(10.0, 10.0)))];
        texts.extend(take_hints());
        assert_eq!(find_label(&texts, "Outline of the note", 1), Some(egui::pos2(25.0, 35.0)));
        assert_eq!(find_label(&texts, "Outline", 1), Some(egui::pos2(105.0, 10.0)), "a drawn text is preferred");
        assert!(take_hints().iter().all(|(t, _)| t != "Outline of the note"), "forgotten once taken");
        *HINTS.lock().unwrap() = None;
    }

    #[test]
    fn a_label_is_found_exactly_before_by_a_part_of_its_text() {
        let r = |x: f32| egui::Rect::from_min_size(egui::pos2(x, 0.0), egui::vec2(10.0, 10.0));
        let texts = vec![("Search mail (from:)".to_string(), r(0.0)), ("Mail".to_string(), r(100.0))];
        assert_eq!(find_label(&texts, "Mail", 1), Some(egui::pos2(105.0, 5.0)));
        assert_eq!(find_label(&texts, "from:", 1), Some(egui::pos2(5.0, 5.0)));
        assert_eq!(find_label(&texts, "Nope", 1), None);
        assert_eq!(find_label(&texts, "Mail", 2), None);
        assert_eq!(find_label(&texts, "ail", 2), Some(egui::pos2(105.0, 5.0)));
    }

    /// One `[name]` section of `docs/screenshots/recipes.conf`.
    struct Recipe {
        name: String,
        keys: std::collections::HashMap<String, String>,
    }

    fn root() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    fn recipes() -> Vec<Recipe> {
        let text = std::fs::read_to_string(root().join("docs/screenshots/recipes.conf")).unwrap();
        let mut out: Vec<Recipe> = vec![];
        for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
            if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                out.push(Recipe { name: name.to_string(), keys: Default::default() });
            } else {
                let (k, v) = line.split_once('=').unwrap_or_else(|| panic!("recipes.conf: `{line}` is neither [name] nor key = value"));
                let r = out.last_mut().expect("recipes.conf: a key before the first [name]");
                assert!(["size", "view", "steps", "vault", "env", "docs"].contains(&k.trim()), "recipes.conf: unknown key `{}` in [{}]", k.trim(), r.name);
                assert!(r.keys.insert(k.trim().to_string(), v.trim().to_string()).is_none(), "recipes.conf: `{}` twice in [{}]", k.trim(), r.name);
            }
        }
        out
    }

    // spec: DEV-007
    #[test]
    fn dev_007_every_screenshot_has_a_recipe_and_every_recipe_a_picture_shown_in_the_docs() {
        let recipes = recipes();
        let mut names: Vec<&str> = recipes.iter().map(|r| r.name.as_str()).collect();
        names.sort_unstable();
        let mut unique = names.clone();
        unique.dedup();
        assert_eq!(names, unique, "a recipe name twice");

        let pictures: std::collections::BTreeSet<String> = std::fs::read_dir(root().join("docs/screenshots")).unwrap().filter_map(|e| e.ok()?.file_name().to_str()?.strip_suffix(".png").map(str::to_string)).collect();
        let recipe_names: std::collections::BTreeSet<String> = recipes.iter().map(|r| r.name.clone()).collect();
        assert_eq!(pictures.difference(&recipe_names).collect::<Vec<_>>(), Vec::<&String>::new(), "a picture without a recipe");
        assert_eq!(recipe_names.difference(&pictures).collect::<Vec<_>>(), Vec::<&String>::new(), "a recipe whose picture is missing (run docs/screenshots/regen.sh <name>)");

        for r in &recipes {
            let docs: Vec<&str> = r.keys.get("docs").map(|d| d.split_whitespace().collect()).unwrap_or_default();
            assert!(!docs.is_empty(), "[{}] says no document that shows it", r.name);
            for d in docs {
                let text = std::fs::read_to_string(root().join(d)).unwrap_or_else(|e| panic!("[{}]: {d}: {e}", r.name));
                assert!(text.contains(&format!("screenshots/{}.png", r.name)), "[{}]: {d} does not show screenshots/{}.png", r.name, r.name);
            }
        }
    }

    // spec: DEV-007
    #[test]
    fn dev_007_every_recipe_parses_and_points_at_files_that_exist() {
        for r in recipes() {
            let k = |key: &str| r.keys.get(key).map(String::as_str);
            if let Some(size) = k("size") {
                assert!(crate::parse_size(size).is_some(), "[{}] size {size}", r.name);
            }
            if let Some(view) = k("view") {
                let known = ["tasks", "graph", "drawing", "reading", "calendar", "calendar-side", "mail", "keybindings", "data-usage", "social", "social-import"];
                assert!(known.contains(&view) || ["open:", "read-only:", "convert:"].iter().any(|p| view.starts_with(p)), "[{}] view {view}", r.name);
            }
            if let Some(v) = k("vault") {
                assert!(root().join("docs/screenshots").join(v).is_dir(), "[{}] vault {v}", r.name);
            }
            let steps = k("steps").unwrap_or("").replace("{fixtures}", root().join("docs/screenshots/fixtures").to_str().unwrap());
            let parsed = parse(&steps).unwrap_or_else(|e| panic!("[{}] steps: {e}", r.name));
            for s in parsed {
                if let Step::Drop(_, paths) = s {
                    for p in paths {
                        assert!(p.is_file(), "[{}] drops {}", r.name, p.display());
                    }
                }
            }
        }
    }

    // spec: DEV-007
    #[test]
    fn dev_007_the_recipes_cover_the_screens_the_new_layout_needs() {
        let recipes = recipes();
        let steps_of = |n: &str| recipes.iter().find(|r| r.name == n).unwrap_or_else(|| panic!("no recipe {n}")).keys.get("steps").cloned().unwrap_or_default();
        // a split right sidebar (LAY-009) needs a drag, a pinned tab (NAV-016) a right click, an open mail a key and a wait
        assert!(steps_of("sidebar-split").starts_with("drag:"));
        assert!(steps_of("tab-pinned-menu").contains("rclick:"));
        assert!(steps_of("mail-view").contains("waitfor:") && steps_of("mail-view").contains("key:Enter"));
        assert!(recipes.iter().any(|r| r.name == "keybindings"));
    }

    // spec: DEV-007
    #[test]
    fn dev_007_the_docs_show_no_picture_that_has_no_recipe() {
        let names: std::collections::BTreeSet<String> = recipes().into_iter().map(|r| r.name).collect();
        for doc in ["README.md", "docs/FEATURES.md", "docs/IMPLEMENTATION.md"] {
            let text = std::fs::read_to_string(root().join(doc)).unwrap();
            for part in text.split("screenshots/").skip(1) {
                if let Some((name, _)) = part.split_once(".png") {
                    if name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
                        assert!(names.contains(name), "{doc} shows screenshots/{name}.png, which has no recipe");
                    }
                }
            }
        }
    }

    // spec: BUILD-007
    #[test]
    fn build_007_a_drop_step_takes_files_and_a_place() {
        let one = |p: &str| vec![PathBuf::from(p)];
        assert_eq!(parse("drop:/tmp/a.png").unwrap(), vec![Step::Drop(None, one("/tmp/a.png"))]);
        assert_eq!(parse("drop:C:\\x\\a.png").unwrap(), vec![Step::Drop(None, one("C:\\x\\a.png"))]);
        assert_eq!(parse("drop:10,20.5:/tmp/a b.md").unwrap(), vec![Step::Drop(Some(Target::At(10.0, 20.5)), one("/tmp/a b.md"))]);
        assert_eq!(parse("drop:@Welcome#2:/a;;/b c").unwrap(), vec![Step::Drop(Some(Target::Label("Welcome".into(), 2)), vec![PathBuf::from("/a"), PathBuf::from("/b c")])]);
        assert!(parse("drop:").is_err() && parse("drop:1,2:").is_err());
    }
}
