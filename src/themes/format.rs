//! The theme file: the colours of a terminal theme in the Alacritty TOML format, read as they
//! are, plus an optional `[shale]` table that sets palette slots exactly (CFG-063).

use crate::theme::Palette;
use eframe::egui::Color32;

/// Index of a colour in [`Terminal::normal`] and [`Terminal::bright`], in the order of the file.
pub const BLACK: usize = 0;
pub const RED: usize = 1;
pub const GREEN: usize = 2;
pub const YELLOW: usize = 3;
pub const BLUE: usize = 4;
pub const MAGENTA: usize = 5;
pub const CYAN: usize = 6;

/// The names of the eight colours, in the order of the indexes above.
const NAMES: [&str; 8] = ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white"];

/// The colours of the terminal theme that Shale uses.
#[derive(Clone, Debug, PartialEq)]
pub struct Terminal {
    pub background: Color32,
    pub foreground: Color32,
    pub normal: [Color32; 8],
    /// The bright colours; one the file lacks is the normal one.
    pub bright: [Color32; 8],
    /// `[colors.selection]` `background`, when it is a colour (not `CellForeground` or
    /// `CellBackground`).
    pub selection: Option<Color32>,
}

/// A theme file read: the terminal colours and what its `[shale]` table says.
#[derive(Clone, Debug, PartialEq)]
pub struct ThemeFile {
    pub terminal: Terminal,
    /// `dark = true|false` of the `[shale]` table.
    pub dark: Option<bool>,
    /// The palette slots the `[shale]` table sets, by name, in alphabetical order.
    pub slots: Vec<(String, Color32)>,
}

/// A colour as a theme file writes it: `'#rrggbb'` or `'0xrrggbb'`, any case. With `alpha` the
/// forms with a last pair of digits (`#rrggbbaa`, not premultiplied) are allowed too.
pub fn parse_colour(text: &str, alpha: bool) -> Result<Color32, String> {
    let t = text.trim();
    let digits = t.strip_prefix('#').or_else(|| t.strip_prefix("0x")).or_else(|| t.strip_prefix("0X")).ok_or_else(|| format!("{text:?} is not a colour (write '#rrggbb' or '0xrrggbb')"))?;
    let ok = digits.is_ascii() && (digits.len() == 6 || (alpha && digits.len() == 8)) && digits.bytes().all(|b| b.is_ascii_hexdigit());
    if !ok {
        return Err(format!("{text:?} is not a colour ({} hex digits after the # or 0x are needed)", if alpha { "6 or 8" } else { "6" }));
    }
    let byte = |i: usize| u8::from_str_radix(&digits[2 * i..2 * i + 2], 16).unwrap_or(0);
    Ok(if digits.len() == 8 { Color32::from_rgba_unmultiplied(byte(0), byte(1), byte(2), byte(3)) } else { Color32::from_rgb(byte(0), byte(1), byte(2)) })
}

/// The value of `key` in `table` as a colour; `None` when it is not there.
fn colour(table: &toml::Table, path: &str, key: &str, alpha: bool) -> Result<Option<Color32>, String> {
    let Some(v) = table.get(key) else { return Ok(None) };
    let text = v.as_str().ok_or_else(|| format!("[{path}] {key} must be a colour in quotes"))?;
    parse_colour(text, alpha).map(Some).map_err(|e| format!("[{path}] {key}: {e}"))
}

/// The table `[colors.<name>]`, if there is one.
fn sub<'a>(colors: &'a toml::Table, name: &str) -> Result<Option<&'a toml::Table>, String> {
    match colors.get(name) {
        None => Ok(None),
        Some(v) => v.as_table().map(Some).ok_or_else(|| format!("[colors.{name}] must be a table")),
    }
}

/// The eight colours of `[colors.<name>]`; each one the file lacks is `fallback`'s, and without a
/// fallback it is an error.
fn eight(colors: &toml::Table, name: &str, fallback: Option<&[Color32; 8]>) -> Result<[Color32; 8], String> {
    let Some(table) = sub(colors, name)? else {
        return match fallback {
            Some(f) => Ok(*f),
            None => Err(format!("[colors.{name}] is missing")),
        };
    };
    let path = format!("colors.{name}");
    let mut out = [Color32::BLACK; 8];
    for (i, n) in NAMES.iter().enumerate() {
        out[i] = match (colour(table, &path, n, false)?, fallback) {
            (Some(c), _) => c,
            (None, Some(f)) => f[i],
            (None, None) => return Err(format!("[{path}] is missing `{n}`")),
        };
    }
    Ok(out)
}

/// Read a theme file. The Alacritty tables `colors.primary` (`background`, `foreground`) and
/// `colors.normal` are required, `colors.bright` and `colors.selection` are optional, everything
/// else of Alacritty's format is ignored; the table `shale` is Shale's own (CFG-063).
pub fn parse(text: &str) -> Result<ThemeFile, String> {
    let root: toml::Table = text.parse().map_err(|e: toml::de::Error| format!("not valid TOML: {}", e.message()))?;
    let colors = root.get("colors").and_then(toml::Value::as_table).ok_or("there is no [colors] table, so this is not an Alacritty theme file")?;
    let primary = sub(colors, "primary")?.ok_or("[colors.primary] is missing")?;
    let need = |key: &str| colour(primary, "colors.primary", key, false)?.ok_or_else(|| format!("[colors.primary] is missing `{key}`"));
    let (background, foreground) = (need("background")?, need("foreground")?);
    let normal = eight(colors, "normal", None)?;
    let bright = eight(colors, "bright", Some(&normal))?;
    let selection = match sub(colors, "selection")? {
        // `CellForeground` and `CellBackground` mean "the cell's own colour": not a colour
        Some(t) if t.get("background").and_then(toml::Value::as_str).is_some_and(|s| matches!(s.trim(), "CellForeground" | "CellBackground")) => None,
        Some(t) => colour(t, "colors.selection", "background", false)?,
        None => None,
    };
    let (mut dark, mut slots) = (None, vec![]);
    if let Some(shale) = root.get("shale") {
        let shale = shale.as_table().ok_or("[shale] must be a table")?;
        for (key, value) in shale {
            if key == "dark" {
                dark = Some(value.as_bool().ok_or("[shale] dark must be true or false")?);
            } else if Palette::SLOTS.contains(&key.as_str()) {
                let text = value.as_str().ok_or_else(|| format!("[shale] {key} must be a colour in quotes"))?;
                slots.push((key.clone(), parse_colour(text, true).map_err(|e| format!("[shale] {key}: {e}"))?));
            } else {
                return Err(format!("[shale] has no setting {key:?} (the slots are dark and {})", Palette::SLOTS.join(", ")));
            }
        }
    }
    Ok(ThemeFile { terminal: Terminal { background, foreground, normal, bright, selection }, dark, slots })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = "[colors.primary]\nbackground = '#101010'\nforeground = '#e0e0e0'\n[colors.normal]\nblack = '#000000'\nred = '#ff0000'\ngreen = '#00ff00'\nyellow = '#ffff00'\nblue = '#0000ff'\nmagenta = '#ff00ff'\ncyan = '#00ffff'\nwhite = '#ffffff'\n";

    fn rgb(r: u8, g: u8, b: u8) -> Color32 {
        Color32::from_rgb(r, g, b)
    }

    // spec: CFG-063
    #[test]
    fn cfg_063_both_hex_syntaxes_in_any_case_are_read() {
        assert_eq!(parse_colour("#1A2b3C", false), Ok(rgb(0x1a, 0x2b, 0x3c)));
        assert_eq!(parse_colour("0x1a2B3c", false), Ok(rgb(0x1a, 0x2b, 0x3c)));
        assert_eq!(parse_colour("0X1A2B3C", false), Ok(rgb(0x1a, 0x2b, 0x3c)));
        for bad in ["", "#12345", "#1234567", "1a2b3c", "#gggggg", "#12345é", "red"] {
            assert!(parse_colour(bad, false).is_err(), "{bad:?}");
        }
        assert!(parse_colour("#11223344", false).is_err(), "no alpha in the terminal tables");
        assert_eq!(parse_colour("#11223344", true), Ok(Color32::from_rgba_unmultiplied(0x11, 0x22, 0x33, 0x44)));
        let text = MINIMAL.replace("'#101010'", "'0x101010'");
        assert_eq!(parse(&text).unwrap().terminal.background, rgb(16, 16, 16));
    }

    // spec: CFG-063
    #[test]
    fn cfg_063_the_required_tables_and_keys_are_primary_and_normal() {
        let t = parse(MINIMAL).unwrap();
        assert_eq!((t.terminal.background, t.terminal.foreground), (rgb(16, 16, 16), rgb(224, 224, 224)));
        assert_eq!(t.terminal.normal[RED], rgb(255, 0, 0));
        for (what, text) in [
            ("no colors", "x = 1".to_string()),
            ("no primary", MINIMAL.replace("[colors.primary]", "[colors.other]")),
            ("no background", MINIMAL.replace("background = '#101010'\n", "")),
            ("no foreground", MINIMAL.replace("foreground = '#e0e0e0'\n", "")),
            ("no normal", MINIMAL.replace("[colors.normal]", "[colors.dim]")),
            ("no red", MINIMAL.replace("red = '#ff0000'\n", "")),
            ("bad colour", MINIMAL.replace("'#ff0000'", "'#ff00'")),
            ("not text", MINIMAL.replace("'#ff0000'", "16711680")),
            ("not toml", "[colors".to_string()),
        ] {
            assert!(parse(&text).is_err(), "{what} is an error");
        }
        assert!(parse(&MINIMAL.replace("red = '#ff0000'\n", "")).unwrap_err().contains("missing `red`"));
    }

    // spec: CFG-063
    #[test]
    fn cfg_063_a_missing_bright_colour_is_the_normal_one() {
        let t = parse(MINIMAL).unwrap();
        assert_eq!(t.terminal.bright, t.terminal.normal, "no [colors.bright] at all");
        let t = parse(&format!("{MINIMAL}[colors.bright]\nred = '#aa0000'\n")).unwrap();
        assert_eq!(t.terminal.bright[RED], rgb(0xaa, 0, 0));
        assert_eq!(t.terminal.bright[GREEN], t.terminal.normal[GREEN], "the others fall back one by one");
        assert!(parse(&format!("{MINIMAL}[colors.bright]\nred = 'nope'\n")).is_err(), "a bad bright colour is an error");
    }

    // spec: CFG-063
    #[test]
    fn cfg_063_selection_is_optional_and_cell_colours_count_as_not_given() {
        assert_eq!(parse(MINIMAL).unwrap().terminal.selection, None);
        assert_eq!(parse(&format!("{MINIMAL}[colors.selection]\ntext = '#000000'\nbackground = '#334455'\n")).unwrap().terminal.selection, Some(rgb(0x33, 0x44, 0x55)));
        for word in ["CellForeground", "CellBackground"] {
            let t = parse(&format!("{MINIMAL}[colors.selection]\ntext = 'CellBackground'\nbackground = '{word}'\n[colors.cursor]\ntext = 'CellBackground'\ncursor = 'CellForeground'\n")).unwrap();
            assert_eq!(t.terminal.selection, None, "{word}");
        }
        assert!(parse(&format!("{MINIMAL}[colors.selection]\nbackground = 'blue'\n")).is_err());
    }

    // spec: CFG-063
    #[test]
    fn cfg_063_other_tables_of_alacritty_are_ignored() {
        let extra = "[colors.search.matches]\nforeground = 'x'\n[colors.hints.start]\nbackground = 1\n[colors.footer_bar]\nbackground = 'x'\n[[colors.indexed_colors]]\nindex = 16\ncolor = '#d18616'\n[colors.vi_mode_cursor]\ncursor = '#fff'\n[font]\nsize = 12\n[colors.primary.extra]\na = 1\n";
        let t = parse(&format!("{MINIMAL}{extra}")).unwrap();
        assert_eq!(t, parse(MINIMAL).unwrap());
    }

    // spec: CFG-063
    #[test]
    fn cfg_063_the_shale_table_sets_slots_and_dark_and_its_errors_are_errors() {
        let t = parse(&format!("{MINIMAL}[shale]\ndark = false\nmuted = '#808080'\nhighlight_bg = '0xffcc0040'\n")).unwrap();
        assert_eq!(t.dark, Some(false));
        assert_eq!(t.slots, [("highlight_bg".to_string(), Color32::from_rgba_unmultiplied(255, 204, 0, 0x40)), ("muted".to_string(), rgb(128, 128, 128))]);
        assert_eq!(parse(MINIMAL).unwrap().dark, None);
        let unknown = parse(&format!("{MINIMAL}[shale]\nmutd = '#808080'\n")).unwrap_err();
        assert!(unknown.contains("mutd") && unknown.contains("muted"), "{unknown}");
        assert!(parse(&format!("{MINIMAL}[shale]\nmuted = '#80'\n")).unwrap_err().contains("muted"));
        assert!(parse(&format!("{MINIMAL}[shale]\nmuted = 5\n")).is_err());
        assert!(parse(&format!("{MINIMAL}[shale]\ndark = 'yes'\n")).is_err());
        assert!(parse(&format!("shale = 1\n{MINIMAL}")).is_err(), "[shale] must be a table");
    }

    // spec: CFG-063
    #[test]
    fn cfg_063_every_slot_of_the_palette_can_be_set_by_its_name() {
        let mut p = Palette::shale();
        for name in Palette::SLOTS {
            *p.slot_mut(name).unwrap_or_else(|| panic!("{name}")) = Color32::from_rgb(1, 2, 3);
        }
        assert!(p.slot_mut("dark").is_none() && p.slot_mut("nothing").is_none());
        let q = Palette::shale();
        assert_eq!(Palette::SLOTS.len(), 45, "a new colour of the palette is a slot too (update this number)");
        assert_eq!(p.bg_primary, Color32::from_rgb(1, 2, 3));
        assert_eq!(p.graph_node_drawing, Color32::from_rgb(1, 2, 3));
        assert_eq!(p.dark, q.dark);
    }
}
