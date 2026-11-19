//! The themes of the window (CFG-062 to CFG-065): Shale's own, the built-in ones (Alacritty theme
//! files in `assets/themes/`) and the files of the user's themes folder; and which one is in use.

mod format;
mod mapping;

use crate::config::ThemeCfg;
use crate::theme::{self, Palette};
use eframe::egui;
use std::path::{Path, PathBuf};

pub use mapping::palette_of;

/// The name of Shale's own theme, the default.
pub const SHALE: &str = "Shale";

/// The built-in themes that come as Alacritty files: name and file text (CFG-062). Shale's own
/// theme is [`Palette::shale`], not a file.
const BUILTIN: [(&str, &str); 8] = [
    ("Dracula", include_str!("../../assets/themes/dracula.toml")),
    ("Nord", include_str!("../../assets/themes/nord.toml")),
    ("Catppuccin Mocha", include_str!("../../assets/themes/catppuccin_mocha.toml")),
    ("Gruvbox Dark", include_str!("../../assets/themes/gruvbox_dark.toml")),
    ("Catppuccin Latte", include_str!("../../assets/themes/catppuccin_latte.toml")),
    ("Solarized Light", include_str!("../../assets/themes/solarized_light.toml")),
    ("GitHub Light", include_str!("../../assets/themes/github_light.toml")),
    ("Gruvbox Light", include_str!("../../assets/themes/gruvbox_light.toml")),
];

/// The largest theme file that is read.
const MAX_FILE: usize = 256 * 1024;

/// A theme to choose.
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    pub name: String,
    pub palette: Palette,
    /// It comes from a file of the themes folder (also one that replaces a built-in).
    pub user: bool,
}

/// All the themes there are, and the files of the themes folder that could not be read.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Themes {
    pub list: Vec<Theme>,
    /// One line per file that was skipped: its name and what is wrong with it (CFG-065).
    pub errors: Vec<String>,
}

/// The themes folder: next to the settings file (CFG-065).
pub fn folder() -> Option<PathBuf> {
    crate::config::config_path().map(|p| p.with_file_name("themes"))
}

/// The theme of a text, as a file of the Alacritty format.
fn from_text(text: &str) -> Result<Palette, String> {
    format::parse(text).map(|f| palette_of(&f))
}

impl Themes {
    /// The built-in themes, and the files of `dir` (CFG-062, CFG-065). A file takes the name of
    /// its file stem and replaces a built-in theme of that name; the others follow in the order
    /// of their names.
    pub fn load(dir: Option<&Path>) -> Themes {
        let mut t = Themes { list: vec![Theme { name: SHALE.into(), palette: Palette::shale(), user: false }], errors: vec![] };
        for (name, text) in BUILTIN {
            // the files are tested to parse; a built-in that did not would only be missing
            if let Ok(palette) = from_text(text) {
                t.list.push(Theme { name: name.into(), palette, user: false });
            }
        }
        let Some(dir) = dir else { return t };
        let Ok(entries) = std::fs::read_dir(dir) else { return t };
        let mut files: Vec<PathBuf> = entries.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.is_file() && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("toml"))).collect();
        files.sort();
        for path in files {
            let (Some(name), Some(file)) = (path.file_stem().map(|s| s.to_string_lossy().into_owned()), path.file_name().map(|s| s.to_string_lossy().into_owned())) else { continue };
            match read(&path).and_then(|text| from_text(&text)) {
                Ok(palette) => {
                    let theme = Theme { name: name.clone(), palette, user: true };
                    match t.list.iter().position(|x| x.name == name) {
                        Some(at) => t.list[at] = theme,
                        None => t.list.push(theme),
                    }
                }
                Err(e) => t.errors.push(format!("{file}: {e}")),
            }
        }
        t
    }

    /// The theme with this name.
    pub fn get(&self, name: &str) -> Option<&Theme> {
        self.list.iter().find(|t| t.name == name)
    }
}

/// The text of a theme file.
fn read(path: &Path) -> Result<String, String> {
    let bytes = crate::fsutil::read(path).map_err(|e| format!("could not be read: {e}"))?;
    if bytes.len() > MAX_FILE {
        return Err(format!("is too big for a theme file ({} KiB at most)", MAX_FILE / 1024));
    }
    String::from_utf8(bytes).map_err(|_| "is not UTF-8 text".to_string())
}

/// The name of the theme the settings ask for, with the system in `system` mode (`None` counts
/// as dark): `name`, or with `follow_system` the light or the dark one.
pub fn wanted(cfg: &ThemeCfg, system: Option<egui::Theme>) -> &str {
    match (cfg.follow_system, system) {
        (false, _) => &cfg.name,
        (true, Some(egui::Theme::Light)) => &cfg.light,
        (true, _) => &cfg.dark,
    }
}

/// The themes there are and the one in use.
pub struct ThemeState {
    /// The themes folder; none for an app that does not use the user's files (tests, screenshots).
    dir: Option<PathBuf>,
    themes: Themes,
    /// Counts the loads, so a theme file that changed is applied again.
    generation: u64,
    /// The name and load of the theme put into the window.
    applied: Option<(String, u64)>,
}

impl ThemeState {
    /// The built-in themes and, with `persist`, those of the themes folder next to the settings.
    pub fn new(persist: bool) -> ThemeState {
        Self::in_folder(if persist { folder() } else { None })
    }

    /// The built-in themes and those of this folder.
    pub fn in_folder(dir: Option<PathBuf>) -> ThemeState {
        ThemeState { themes: Themes::load(dir.as_deref()), dir, generation: 0, applied: None }
    }

    /// The themes folder.
    pub fn dir(&self) -> Option<&Path> {
        self.dir.as_deref()
    }

    pub fn themes(&self) -> &Themes {
        &self.themes
    }

    /// Read the themes folder again (CFG-065).
    pub fn reload(&mut self) {
        self.themes = Themes::load(self.dir.as_deref());
        self.generation += 1;
    }

    /// Put the theme the settings ask for into the window when it is not in yet: another name,
    /// another system mode, a file that was read again. An unknown name gives Shale (CFG-062).
    pub fn sync(&mut self, ctx: &egui::Context, cfg: &ThemeCfg) {
        let name = wanted(cfg, ctx.system_theme());
        if self.applied.as_ref().is_some_and(|(n, g)| n == name && *g == self.generation) {
            return;
        }
        let palette = self.themes.get(name).map_or_else(Palette::shale, |t| t.palette);
        theme::switch_palette(ctx, palette);
        self.applied = Some((name.to_string(), self.generation));
        ctx.request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Color32, RawInput, Theme};

    /// A folder of its own in the temp folder.
    fn temp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("shale-themes-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    const MINE: &str = "[colors.primary]\nbackground = '#ffffff'\nforeground = '#222222'\n[colors.normal]\nblack = '#000000'\nred = '#cc0000'\ngreen = '#00aa00'\nyellow = '#aaaa00'\nblue = '#0000cc'\nmagenta = '#aa00aa'\ncyan = '#00aaaa'\nwhite = '#cccccc'\n";

    // spec: CFG-062
    #[test]
    fn cfg_062_the_built_in_themes_are_these_and_light_or_dark_as_listed() {
        let themes = Themes::load(None);
        let names: Vec<&str> = themes.list.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["Shale", "Dracula", "Nord", "Catppuccin Mocha", "Gruvbox Dark", "Catppuccin Latte", "Solarized Light", "GitHub Light", "Gruvbox Light"]);
        assert!(themes.errors.is_empty() && themes.list.iter().all(|t| !t.user));
        let dark = ["Shale", "Dracula", "Nord", "Catppuccin Mocha", "Gruvbox Dark"];
        for t in &themes.list {
            assert_eq!(t.palette.dark, dark.contains(&t.name.as_str()), "{}", t.name);
        }
        assert_eq!(themes.get("Shale").unwrap().palette, Palette::shale(), "Shale is the palette in code");
        assert!(themes.get("Nothing").is_none());
    }

    // spec: CFG-062
    #[test]
    fn cfg_062_every_built_in_file_parses_and_reads_well() {
        for (name, text) in BUILTIN {
            let file = format::parse(text).unwrap_or_else(|e| panic!("{name}: {e}"));
            let p = palette_of(&file);
            let bg = p.bg_primary;
            assert_eq!(bg, file.terminal.background, "{name}");
            for (what, c, min) in [
                ("text", p.text, 4.5),
                ("heading", p.heading, 4.5),
                ("muted", p.muted, 3.0),
                ("faint", p.faint, 3.0),
                ("comment", p.syntax_comment, 3.0),
                ("link", p.link, 3.0),
                ("missing link", p.link_missing, 3.0),
                ("code", p.code, 3.0),
                ("string", p.syntax_string, 3.0),
            ] {
                assert!(mapping::contrast(c, bg) >= min, "{name}: {what} {c:?} on {bg:?} is {}", mapping::contrast(c, bg));
            }
            assert_ne!(p.accent, p.bg_primary, "{name}");
            assert!(p.selection_bg != bg && mapping::contrast(p.selection_bg, p.text) >= 3.0, "{name}: selected text can be read");
        }
    }

    // spec: CFG-062, CFG-063
    #[test]
    fn cfg_062_only_solarized_light_has_a_shale_table_and_it_sets_the_comment_colour() {
        for (name, text) in BUILTIN {
            let file = format::parse(text).unwrap();
            if name == "Solarized Light" {
                assert_eq!(file.slots, [("syntax_comment".to_string(), Color32::from_rgb(0x7c, 0x8d, 0x90))]);
                assert_eq!(palette_of(&file).syntax_comment, Color32::from_rgb(0x7c, 0x8d, 0x90));
            } else {
                assert!(file.slots.is_empty() && file.dark.is_none(), "{name}");
            }
        }
    }

    // spec: CFG-062
    #[test]
    fn cfg_062_the_settings_keys_are_in_the_theme_table_with_their_defaults() {
        let c = crate::config::Config::default();
        assert_eq!(c.theme, ThemeCfg { name: "Shale".into(), follow_system: false, light: "Catppuccin Latte".into(), dark: "Shale".into() });
        let text = c.file_text();
        for line in ["[theme]", "name = \"Shale\"", "follow_system = false", "light = \"Catppuccin Latte\"", "dark = \"Shale\""] {
            assert!(text.lines().any(|l| l == line), "{line} in\n{text}");
        }
        assert!(text.contains("# true: the theme follows the system's light or dark mode"), "{text}");
        let mut c = c;
        c.theme = ThemeCfg { name: "Nord".into(), follow_system: true, light: "GitHub Light".into(), dark: "Dracula".into() };
        let (back, _) = crate::config::Config::parse_text(&c.file_text()).unwrap();
        assert_eq!(back.theme, c.theme, "the keys round-trip");
        let (old, _) = crate::config::Config::parse_text("toolbar_size = 18\n").unwrap();
        assert_eq!(old.theme, ThemeCfg::default(), "an older file has the defaults");
        let (part, _) = crate::config::Config::parse_text("[theme]\nname = \"Nord\"\n").unwrap();
        assert_eq!((part.theme.name.as_str(), part.theme.dark.as_str()), ("Nord", "Shale"), "a key left out has its default");
    }

    // spec: CFG-062
    #[test]
    fn cfg_062_follow_system_picks_the_light_or_the_dark_theme_and_none_counts_as_dark() {
        let cfg = ThemeCfg { name: "Nord".into(), follow_system: true, light: "GitHub Light".into(), dark: "Dracula".into() };
        assert_eq!(wanted(&cfg, Some(Theme::Light)), "GitHub Light");
        assert_eq!(wanted(&cfg, Some(Theme::Dark)), "Dracula");
        assert_eq!(wanted(&cfg, None), "Dracula");
        let one = ThemeCfg { follow_system: false, ..cfg };
        for s in [None, Some(Theme::Light), Some(Theme::Dark)] {
            assert_eq!(wanted(&one, s), "Nord", "the single theme ignores the system");
        }
    }

    /// One frame with the system in this mode.
    fn frame(ctx: &egui::Context, system: Option<Theme>) {
        let mut out = ctx.run_ui(RawInput { system_theme: system, ..Default::default() }, |_| {});
        out.textures_delta.clear();
    }

    // spec: CFG-062
    #[test]
    fn cfg_062_sync_applies_a_theme_at_once_follows_the_system_and_applies_again_only_on_a_change() {
        let ctx = egui::Context::default();
        let mut state = ThemeState::in_folder(None);
        let mut cfg = ThemeCfg::default();
        frame(&ctx, Some(Theme::Light));
        state.sync(&ctx, &cfg);
        assert_eq!(theme::palette(), Palette::shale(), "Shale is the default, whatever the system");
        assert!(ctx.global_style().visuals.dark_mode);
        cfg.name = "Nord".into();
        state.sync(&ctx, &cfg);
        assert_eq!(theme::palette(), state.themes().get("Nord").unwrap().palette, "a change applies at once");
        assert_eq!(ctx.global_style().visuals.panel_fill, theme::palette().bg_secondary);
        // an edit of the palette stays: nothing applies again while nothing changed
        let mut other = theme::palette();
        other.accent = Color32::RED;
        theme::set_palette(other);
        state.sync(&ctx, &cfg);
        assert_eq!(theme::palette().accent, Color32::RED, "no second apply");
        // following the system
        cfg.follow_system = true;
        cfg.light = "Catppuccin Latte".into();
        cfg.dark = "Dracula".into();
        frame(&ctx, Some(Theme::Light));
        state.sync(&ctx, &cfg);
        assert_eq!(theme::palette(), state.themes().get("Catppuccin Latte").unwrap().palette);
        assert!(!ctx.global_style().visuals.dark_mode);
        frame(&ctx, Some(Theme::Dark));
        state.sync(&ctx, &cfg);
        assert_eq!(theme::palette(), state.themes().get("Dracula").unwrap().palette);
        assert!(ctx.global_style().visuals.dark_mode);
        frame(&ctx, None);
        state.sync(&ctx, &cfg);
        assert_eq!(theme::palette(), state.themes().get("Dracula").unwrap().palette, "no system mode counts as dark");
        // an unknown name gives Shale
        cfg.follow_system = false;
        cfg.name = "No such theme".into();
        state.sync(&ctx, &cfg);
        assert_eq!(theme::palette(), Palette::shale());
        // a reload applies the theme again
        state.reload();
        theme::set_palette(other);
        state.sync(&ctx, &cfg);
        assert_eq!(theme::palette(), Palette::shale());
        theme::set_palette(Palette::shale());
    }

    // spec: CFG-062
    #[test]
    fn cfg_062_egui_never_swaps_our_visuals_when_the_system_changes_its_mode() {
        let ctx = egui::Context::default();
        let light = Themes::load(None).get("Catppuccin Latte").unwrap().palette;
        frame(&ctx, Some(Theme::Dark));
        theme::switch_palette(&ctx, light);
        let panel = ctx.global_style().visuals.panel_fill;
        assert_eq!(panel, light.bg_secondary);
        for system in [Some(Theme::Dark), Some(Theme::Light), None, Some(Theme::Dark)] {
            frame(&ctx, system);
            assert_eq!(ctx.global_style().visuals.panel_fill, panel, "{system:?}");
            assert!(!ctx.global_style().visuals.dark_mode);
            assert_eq!(ctx.theme(), Theme::Light, "egui uses the style of the palette");
        }
        theme::switch_palette(&ctx, Palette::shale());
        frame(&ctx, Some(Theme::Light));
        assert!(ctx.global_style().visuals.dark_mode, "a dark theme stays dark in a light system");
        assert_eq!(ctx.global_style().visuals.panel_fill, Palette::shale().bg_secondary);
        // the window decorations are left to the system, whose mode we read
        assert!(!ctx.options(|o| o.sync_window_theme));
        theme::set_palette(Palette::shale());
    }

    // spec: CFG-062
    #[test]
    fn cfg_062_changing_the_theme_keeps_the_text_styles_and_the_cursor_settings() {
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        ctx.global_style_mut(|s| {
            s.text_styles.insert(egui::TextStyle::Body, egui::FontId::proportional(21.0));
            s.visuals.text_cursor.blink = false;
            s.animation_time = 0.0;
        });
        theme::switch_palette(&ctx, Themes::load(None).get("Nord").unwrap().palette);
        theme::switch_palette(&ctx, Themes::load(None).get("GitHub Light").unwrap().palette);
        let s = ctx.global_style();
        assert_eq!(s.text_styles[&egui::TextStyle::Body].size, 21.0);
        assert!(!s.visuals.text_cursor.blink && s.animation_time == 0.0);
        theme::set_palette(Palette::shale());
    }

    // spec: CFG-065
    #[test]
    fn cfg_065_the_files_of_the_folder_are_themes_by_their_file_stem_and_a_built_in_name_replaces_it() {
        let dir = temp("user");
        std::fs::write(dir.join("My Theme.toml"), MINE).unwrap();
        std::fs::write(dir.join("Another.TOML"), MINE).unwrap();
        std::fs::write(dir.join("Dracula.toml"), MINE).unwrap();
        std::fs::write(dir.join("notes.txt"), "not a theme").unwrap();
        std::fs::create_dir(dir.join("Folder.toml")).unwrap();
        let themes = Themes::load(Some(&dir));
        let names: Vec<&str> = themes.list.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(
            names,
            ["Shale", "Dracula", "Nord", "Catppuccin Mocha", "Gruvbox Dark", "Catppuccin Latte", "Solarized Light", "GitHub Light", "Gruvbox Light", "Another", "My Theme"],
            "built-ins first (Dracula in place), then the files by name"
        );
        assert!(themes.errors.is_empty(), "{:?}", themes.errors);
        let mine = themes.get("My Theme").unwrap();
        assert!(mine.user && !mine.palette.dark, "a light theme file");
        let dracula = themes.get("Dracula").unwrap();
        assert!(dracula.user, "the file replaced the built-in");
        assert_eq!(dracula.palette, mine.palette);
        assert_eq!(themes.list.iter().filter(|t| t.name == "Dracula").count(), 1);
        assert!(themes.get("Nord").is_some_and(|t| !t.user));
        assert!(themes.get("notes").is_none() && themes.get("Folder").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    // spec: CFG-065
    #[test]
    fn cfg_065_a_file_that_does_not_parse_is_skipped_and_its_error_is_reported() {
        let dir = temp("broken");
        std::fs::write(dir.join("Good.toml"), MINE).unwrap();
        std::fs::write(dir.join("Bad.toml"), MINE.replace("red", "rred")).unwrap();
        std::fs::write(dir.join("Colour.toml"), MINE.replace("'#cc0000'", "'red'")).unwrap();
        std::fs::write(dir.join("Empty.toml"), "").unwrap();
        std::fs::write(dir.join("Binary.toml"), [0xff, 0xfe, 0x00]).unwrap();
        std::fs::write(dir.join("Huge.toml"), vec![b'#'; MAX_FILE + 1]).unwrap();
        std::fs::write(dir.join("Shale override.toml"), format!("{MINE}[shale]\nbogus = '#000000'\n")).unwrap();
        let themes = Themes::load(Some(&dir));
        assert!(themes.get("Good").is_some());
        for bad in ["Bad", "Colour", "Empty", "Binary", "Huge", "Shale override"] {
            assert!(themes.get(bad).is_none(), "{bad} is skipped");
            assert!(themes.errors.iter().any(|e| e.starts_with(&format!("{bad}.toml: "))), "{bad} is reported: {:?}", themes.errors);
        }
        assert_eq!(themes.errors.len(), 6);
        assert!(themes.errors.iter().any(|e| e.contains("missing `red`")) && themes.errors.iter().any(|e| e.contains("bogus")));
        // a broken file named like a built-in leaves the built-in
        std::fs::write(dir.join("Nord.toml"), "x = ").unwrap();
        let again = Themes::load(Some(&dir));
        assert!(again.get("Nord").is_some_and(|t| !t.user));
        assert!(again.errors.iter().any(|e| e.starts_with("Nord.toml: ")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    // spec: CFG-065
    #[test]
    fn cfg_065_a_missing_folder_is_no_themes_and_no_error_and_reload_reads_it_again() {
        let dir = temp("reload");
        let missing = dir.join("nothing");
        assert_eq!(Themes::load(Some(&missing)), Themes::load(None));
        // the folder is next to the settings file; reload picks up a file that was added
        crate::config::CONFIG_TEST_PATH.with(|c| *c.borrow_mut() = Some(dir.join("config.toml")));
        assert_eq!(folder(), Some(dir.join("themes")));
        let mut state = ThemeState::new(true);
        assert_eq!(state.dir(), Some(dir.join("themes").as_path()));
        assert!(ThemeState::new(false).dir().is_none(), "an app without persistence reads no user files");
        assert!(state.themes().get("Added").is_none());
        std::fs::create_dir_all(dir.join("themes")).unwrap();
        std::fs::write(dir.join("themes").join("Added.toml"), MINE).unwrap();
        assert!(state.themes().get("Added").is_none(), "not before the reload");
        state.reload();
        assert!(state.themes().get("Added").is_some_and(|t| t.user));
        crate::config::CONFIG_TEST_PATH.with(|c| *c.borrow_mut() = None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
