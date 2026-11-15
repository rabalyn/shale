//! Where the icons of the window are: the model behind the ribbon (the left menubar) and, later,
//! the views of the right sidebar (LAY-001…). No drawing here, so the rules can be tested alone.
//! The drawing and the dragging are in `sidebar_ui.rs`.

use crate::config::{Config, SidebarCfg};
use crate::layout::Icon;
use std::collections::HashSet;

/// Something that has an icon in a menubar.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Item {
    Files,
    Search,
    Tags,
    Switcher,
    Graph,
    Calendar,
    Mail,
    Tasks,
    Social,
    Import,
    Drawing,
    NewNote,
}

impl Item {
    /// All items in their default order.
    pub const ALL: [Item; 12] = [Item::Files, Item::Search, Item::Tags, Item::Switcher, Item::Graph, Item::Calendar, Item::Mail, Item::Tasks, Item::Social, Item::Import, Item::Drawing, Item::NewNote];

    /// The name stored in the settings. Never change one: stored layouts name them.
    pub fn id(self) -> &'static str {
        match self {
            Item::Files => "files",
            Item::Search => "search",
            Item::Tags => "tags",
            Item::Switcher => "switcher",
            Item::Graph => "graph",
            Item::Calendar => "calendar",
            Item::Mail => "mail",
            Item::Tasks => "tasks",
            Item::Social => "social",
            Item::Import => "import",
            Item::Drawing => "drawing",
            Item::NewNote => "new-note",
        }
    }

    pub fn from_id(id: &str) -> Option<Item> {
        Item::ALL.into_iter().find(|i| i.id() == id)
    }

    pub fn icon(self) -> Icon {
        match self {
            Item::Files => Icon::Files,
            Item::Search => Icon::Search,
            Item::Tags => Icon::Tags,
            Item::Switcher => Icon::Switcher,
            Item::Graph => Icon::Graph,
            Item::Calendar => Icon::Calendar,
            Item::Mail => Icon::Mail,
            Item::Tasks => Icon::Tasks,
            Item::Social => Icon::Social,
            Item::Import => Icon::Import,
            Item::Drawing => Icon::Drawing,
            Item::NewNote => Icon::NewNote,
        }
    }

    pub fn tip(self) -> &'static str {
        match self {
            Item::Files => "Files",
            Item::Search => "Search all notes (Ctrl+Shift+F)",
            Item::Tags => "Tags",
            Item::Switcher => "Quick switcher (Ctrl+O)",
            Item::Graph => "Graph of all notes",
            Item::Calendar => "Calendar",
            Item::Mail => "Mail (Ctrl+Shift+M)",
            Item::Tasks => "To-do list across all notes",
            Item::Social => "Feeds: read Mastodon and Bluesky and save posts to the vault",
            Item::Import => "Import e-mails, events and files (or drop them on the window)",
            Item::Drawing => "New drawing",
            Item::NewNote => "New note (Ctrl+N)",
        }
    }
}

/// The icons of the window, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    pub ribbon: Vec<Item>,
}

impl Default for Layout {
    fn default() -> Self {
        Layout { ribbon: Item::ALL.to_vec() }
    }
}

impl Layout {
    /// The layout of the settings. Whatever is wrong with it is repaired (LAY-002): unknown ids
    /// and doubles go, missing icons come back at the end in their default order.
    pub fn from_config(cfg: &Config) -> Layout {
        let mut seen = HashSet::new();
        let mut ribbon: Vec<Item> = cfg.sidebars.ribbon.iter().filter_map(|id| Item::from_id(id)).filter(|i| seen.insert(*i)).collect();
        ribbon.extend(Item::ALL.into_iter().filter(|i| seen.insert(*i)));
        Layout { ribbon }
    }

    /// Write the layout into the settings.
    pub fn store(&self, cfg: &mut Config) {
        cfg.sidebars = SidebarCfg { ribbon: self.ribbon.iter().map(|i| i.id().to_string()).collect() };
    }

    /// Move a ribbon icon so that it has `at` icons above it (`at` counts the list as it was
    /// before the move).
    pub fn move_in_ribbon(&mut self, item: Item, at: usize) {
        let Some(from) = self.ribbon.iter().position(|i| *i == item) else { return };
        self.ribbon.remove(from);
        let at = if from < at { at - 1 } else { at };
        self.ribbon.insert(at.min(self.ribbon.len()), item);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg_with(ids: &[&str]) -> Config {
        Config { sidebars: SidebarCfg { ribbon: ids.iter().map(|s| s.to_string()).collect() }, ..Config::default() }
    }

    // spec: LAY-002
    #[test]
    fn lay_002_a_stored_order_with_unknown_missing_and_double_ids_is_repaired() {
        assert_eq!(Layout::from_config(&Config::default()), Layout::default(), "nothing stored: the default");
        let l = Layout::from_config(&cfg_with(&["mail", "bogus", "files", "mail", "", "new-note"]));
        assert_eq!(&l.ribbon[..3], [Item::Mail, Item::Files, Item::NewNote], "the stored order is kept");
        let rest: Vec<Item> = Item::ALL.into_iter().filter(|i| ![Item::Mail, Item::Files, Item::NewNote].contains(i)).collect();
        assert_eq!(&l.ribbon[3..], rest.as_slice(), "icons the list lacks come at the end in default order");
        assert_eq!(l.ribbon.len(), Item::ALL.len());
        let mut c = Config::default();
        l.store(&mut c);
        assert_eq!(Layout::from_config(&c), l, "what is stored comes back as it was");
        for i in Item::ALL {
            assert_eq!(Item::from_id(i.id()), Some(i));
        }
    }

    #[test]
    fn moving_in_the_ribbon_counts_the_icons_above_the_drop_point() {
        let mut l = Layout::default();
        l.move_in_ribbon(Item::Files, 3);
        assert_eq!(&l.ribbon[..3], [Item::Search, Item::Tags, Item::Files], "down: the list shrinks above the point");
        l.move_in_ribbon(Item::NewNote, 0);
        assert_eq!(l.ribbon[0], Item::NewNote);
        l.move_in_ribbon(Item::Search, 99);
        assert_eq!(l.ribbon.last(), Some(&Item::Search));
        let before = l.clone();
        l.move_in_ribbon(Item::NewNote, 1);
        assert_eq!(l, before, "dropping an icon right after itself changes nothing");
        assert_eq!(l.ribbon.len(), Item::ALL.len());
    }
}
