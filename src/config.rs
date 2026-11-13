//! App settings, saved as JSON in the user's config folder (~/.config/shale/config.json).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// How the file tree is ordered.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FileSort {
    #[default]
    NameAsc,
    NameDesc,
    CreatedAsc,
    CreatedDesc,
    ModifiedAsc,
    ModifiedDesc,
}

impl FileSort {
    pub const ALL: [FileSort; 6] = [FileSort::NameAsc, FileSort::NameDesc, FileSort::ModifiedDesc, FileSort::ModifiedAsc, FileSort::CreatedDesc, FileSort::CreatedAsc];
    pub fn label(self) -> &'static str {
        match self {
            FileSort::NameAsc => "File name (A to Z)",
            FileSort::NameDesc => "File name (Z to A)",
            FileSort::ModifiedDesc => "Modified (new to old)",
            FileSort::ModifiedAsc => "Modified (old to new)",
            FileSort::CreatedDesc => "Created (new to old)",
            FileSort::CreatedAsc => "Created (old to new)",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Config {
    /// The vault opened last.
    pub vault: Option<PathBuf>,
    pub recent: Vec<PathBuf>,
    /// Follow links only with Ctrl+click (plain click edits). Default: plain click follows.
    pub ctrl_to_follow: bool,
    /// Notes created from a red link go next to the current note instead of the vault root.
    pub new_notes_next_to_current: bool,
    /// Rewrite links in other notes when a note is renamed.
    pub update_links_on_rename: bool,
    /// Draw on a dark canvas.
    pub dark_canvas: bool,
    pub font_size: f32,
    /// Keep the text in a comfortable column instead of the full width.
    pub readable_width: bool,
    pub show_left: bool,
    pub show_right: bool,
    /// Notes open when the app was closed (vault-relative, '/' separated).
    pub open_files: Vec<String>,
    /// Order of the file tree.
    pub file_sort: FileSort,
    /// Show embedded images as pictures inside the editor (the link shows when the cursor is on it).
    pub inline_images: bool,
    /// Calendars shown in the calendar panel (local .ics files and CalDAV calendars).
    pub calendars: Vec<crate::cal_store::CalendarSource>,
    /// Calendar weeks start on Monday (otherwise Sunday).
    pub week_starts_monday: bool,
    /// Troubleshooting: record every key, typed text and input-method event to a file.
    pub input_log: bool,
    /// The folder used for the last import (vault-relative).
    pub last_import_folder: String,
    /// Tags used for imports, most recent first.
    pub recent_tags: Vec<String>,
    /// Folder of the contact notes (for `[[p …` links and people found when importing).
    pub people_dir: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            vault: None,
            recent: vec![],
            ctrl_to_follow: false,
            new_notes_next_to_current: false,
            update_links_on_rename: true,
            dark_canvas: true,
            font_size: 15.5,
            readable_width: true,
            show_left: true,
            show_right: true,
            open_files: vec![],
            file_sort: FileSort::NameAsc,
            inline_images: true,
            calendars: vec![],
            week_starts_monday: true,
            input_log: false,
            last_import_folder: String::new(),
            recent_tags: vec![],
            people_dir: crate::people::DEFAULT_DIR.into(),
        }
    }
}

/// Where the troubleshooting key recording goes (~/.cache/shale/input-debug.log).
pub fn input_log_path() -> PathBuf {
    dirs::cache_dir().unwrap_or_else(std::env::temp_dir).join("shale").join("input-debug.log")
}

/// When this program was built, as "YYYY-MM-DD HH:MM UTC".
pub fn build_stamp() -> String {
    let secs: i64 = env!("SHALE_BUILD_SECS").parse().unwrap_or(0);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02} UTC", rem / 3600, rem % 3600 / 60)
}

pub fn config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("shale").join("config.json"))
}

impl Config {
    pub fn load() -> Config {
        config_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        if let Some(p) = config_path() {
            if let Some(dir) = p.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Ok(s) = serde_json::to_string_pretty(self) {
                let _ = std::fs::write(&p, s);
                // the file may hold CalDAV passwords: only the owner may read it
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600));
                }
            }
        }
    }

    pub fn remember_vault(&mut self, path: PathBuf) {
        self.recent.retain(|p| p != &path);
        self.recent.insert(0, path.clone());
        self.recent.truncate(8);
        self.vault = Some(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_config_files_still_load() {
        let c: Config = serde_json::from_str(r#"{"vault": "/tmp/v"}"#).unwrap();
        assert_eq!(c.vault, Some(PathBuf::from("/tmp/v")));
        assert!(c.update_links_on_rename);
        assert!(!c.ctrl_to_follow);
        assert!(c.inline_images);
        assert_eq!(c.file_sort, FileSort::NameAsc);
    }

    #[test]
    fn recent_list_is_unique() {
        let mut c = Config::default();
        c.remember_vault("/a".into());
        c.remember_vault("/b".into());
        c.remember_vault("/a".into());
        assert_eq!(c.recent, vec![PathBuf::from("/a"), PathBuf::from("/b")]);
    }
}
