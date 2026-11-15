//! Where the icons of the window are: the model behind the ribbon (the left menubar) and the
//! views of the right sidebar (LAY-001…, ADR-0014). No drawing here, so the rules can be tested alone.
//! The drawing and the dragging are in `sidebar_ui.rs`.

use crate::app::RightPanel;
use crate::config::{Config, RightViewCfg, SidebarCfg};
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
    /// something the right sidebar shows; in the ribbon it is a shortcut to that
    Panel(RightPanel),
}

impl Item {
    /// All items in their default order: the ribbon's, then the panels'.
    pub const ALL: [Item; 17] = [Item::Files, Item::Search, Item::Tags, Item::Switcher, Item::Graph, Item::Calendar, Item::Mail, Item::Tasks, Item::Social, Item::Import, Item::Drawing, Item::NewNote, Item::Panel(RightPanel::Backlinks), Item::Panel(RightPanel::Outgoing), Item::Panel(RightPanel::Outline), Item::Panel(RightPanel::LocalGraph), Item::Panel(RightPanel::Calendar)];

    /// The panels, in the default order of the right sidebar's menubar.
    pub fn is_panel(self) -> bool {
        matches!(self, Item::Panel(_))
    }

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
            Item::Panel(RightPanel::Backlinks) => "panel:backlinks",
            Item::Panel(RightPanel::Outgoing) => "panel:outgoing",
            Item::Panel(RightPanel::Outline) => "panel:outline",
            Item::Panel(RightPanel::LocalGraph) => "panel:graph",
            Item::Panel(RightPanel::Calendar) => "panel:calendar",
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
            Item::Panel(RightPanel::Backlinks) => Icon::Backlinks,
            Item::Panel(RightPanel::Outgoing) => Icon::Outgoing,
            Item::Panel(RightPanel::Outline) => Icon::Outline,
            Item::Panel(RightPanel::LocalGraph) => Icon::Graph,
            Item::Panel(RightPanel::Calendar) => Icon::Calendar,
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
            Item::Panel(RightPanel::Backlinks) => "Backlinks and unlinked mentions",
            Item::Panel(RightPanel::Outgoing) => "Outgoing links",
            Item::Panel(RightPanel::Outline) => "Outline of the note",
            Item::Panel(RightPanel::LocalGraph) => "Graph around the note",
            Item::Panel(RightPanel::Calendar) => "Calendar",
        }
    }
}

/// One view of the right sidebar: a menubar of panel icons and the panel of the active one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct View {
    pub icons: Vec<Item>,
    pub active: Item,
}

/// Where an icon is dropped. Counts are "this many icons (views) above or before it", in the
/// layout as it is before the move.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dest {
    Ribbon(usize),
    Bar { view: usize, at: usize },
    NewView { at: usize },
}

/// A right menubar takes panel icons only (LAY-006).
#[derive(Debug, PartialEq, Eq)]
pub struct Refused;

/// The icons of the window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    pub ribbon: Vec<Item>,
    pub views: Vec<View>,
}

impl Default for Layout {
    fn default() -> Self {
        let panels: Vec<Item> = Item::ALL.into_iter().filter(|i| i.is_panel()).collect();
        Layout { ribbon: Item::ALL.into_iter().filter(|i| !i.is_panel()).collect(), views: vec![View { active: panels[0], icons: panels }] }
    }
}

impl Layout {
    /// The layout of the settings. Whatever is wrong with it is repaired (LAY-002): unknown ids,
    /// doubles and ribbon items in a view go; missing icons come back (ribbon items at the end
    /// of the ribbon, panels at the end of the last view); nothing stored gives the default.
    pub fn from_config(cfg: &Config) -> Layout {
        if cfg.sidebars == SidebarCfg::default() {
            return Layout::default();
        }
        let mut seen = HashSet::new();
        let mut views: Vec<View> = vec![];
        for v in &cfg.sidebars.right {
            let icons: Vec<Item> = v.icons.iter().filter_map(|id| Item::from_id(id)).filter(|i| i.is_panel() && seen.insert(*i)).collect();
            if let Some(first) = icons.first().copied() {
                let active = Item::from_id(&v.active).filter(|a| icons.contains(a)).unwrap_or(first);
                views.push(View { icons, active });
            }
        }
        let mut ribbon: Vec<Item> = cfg.sidebars.ribbon.iter().filter_map(|id| Item::from_id(id)).filter(|i| seen.insert(*i)).collect();
        for item in Item::ALL.into_iter().filter(|i| seen.insert(*i)) {
            match (item.is_panel(), views.last_mut()) {
                (false, _) => ribbon.push(item),
                (true, Some(v)) => v.icons.push(item),
                (true, None) => views.push(View { icons: vec![item], active: item }),
            }
        }
        Layout { ribbon, views }
    }

    /// Write the layout into the settings.
    pub fn store(&self, cfg: &mut Config) {
        let ids = |v: &[Item]| v.iter().map(|i| i.id().to_string()).collect();
        cfg.sidebars = SidebarCfg { ribbon: ids(&self.ribbon), right: self.views.iter().map(|v| RightViewCfg { icons: ids(&v.icons), active: v.active.id().to_string() }).collect() };
    }

    /// Move an icon to a place (LAY-005). A right menubar or a new view takes panel icons only
    /// (LAY-006). A view that loses its last icon is removed (LAY-007); an icon that arrives in
    /// a view becomes its active one.
    pub fn apply(&mut self, item: Item, dest: Dest) -> Result<(), Refused> {
        if !item.is_panel() && !matches!(dest, Dest::Ribbon(_)) {
            return Err(Refused);
        }
        let from_ribbon = self.ribbon.iter().position(|i| *i == item);
        let from_view = self.views.iter().enumerate().find_map(|(v, w)| w.icons.iter().position(|i| *i == item).map(|p| (v, p)));
        // an only icon dropped on its own view: it would leave and come back
        if let Some((v, _)) = from_view {
            let same = match dest {
                Dest::Bar { view, .. } => view == v,
                Dest::NewView { at } => at == v || at == v + 1,
                Dest::Ribbon(_) => false,
            };
            if same && self.views[v].icons.len() == 1 {
                return Ok(());
            }
        }
        let mut dest = dest;
        if let Some(p) = from_ribbon {
            self.ribbon.remove(p);
            if let Dest::Ribbon(at) = &mut dest {
                if p < *at {
                    *at -= 1;
                }
            }
        }
        if let Some((v, p)) = from_view {
            self.views[v].icons.remove(p);
            if let Dest::Bar { view, at } = &mut dest {
                if *view == v && p < *at {
                    *at -= 1;
                }
            }
            if self.views[v].icons.is_empty() {
                self.views.remove(v);
                match &mut dest {
                    Dest::Bar { view, .. } if *view > v => *view -= 1,
                    Dest::NewView { at } if *at > v => *at -= 1,
                    _ => {}
                }
            } else if self.views[v].active == item {
                self.views[v].active = self.views[v].icons[0];
            }
        }
        match dest {
            Dest::Ribbon(at) => self.ribbon.insert(at.min(self.ribbon.len()), item),
            Dest::Bar { view, at } => {
                let v = &mut self.views[view];
                v.icons.insert(at.min(v.icons.len()), item);
                v.active = item;
            }
            Dest::NewView { at } => self.views.insert(at.min(self.views.len()), View { icons: vec![item], active: item }),
        }
        Ok(())
    }

    /// Show a panel in the view that has it (LAY-003). False: no view has it.
    pub fn show_panel(&mut self, panel: RightPanel) -> bool {
        let item = Item::Panel(panel);
        match self.views.iter_mut().find(|v| v.icons.contains(&item)) {
            Some(v) => {
                v.active = item;
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use RightPanel::*;

    fn p(x: RightPanel) -> Item {
        Item::Panel(x)
    }

    fn view(icons: &[RightPanel]) -> View {
        View { icons: icons.iter().map(|x| p(*x)).collect(), active: p(icons[0]) }
    }

    fn two_views() -> Layout {
        Layout { ribbon: Layout::default().ribbon, views: vec![view(&[Backlinks, Outline]), view(&[Outgoing, LocalGraph, Calendar])] }
    }

    fn cfg_with(ribbon: &[&str], views: &[(&[&str], &str)]) -> Config {
        let ids = |v: &[&str]| v.iter().map(|s| s.to_string()).collect();
        Config { sidebars: SidebarCfg { ribbon: ids(ribbon), right: views.iter().map(|(i, a)| RightViewCfg { icons: ids(i), active: a.to_string() }).collect() }, ..Config::default() }
    }

    fn every_icon_once(l: &Layout) {
        let all: Vec<Item> = l.ribbon.iter().copied().chain(l.views.iter().flat_map(|v| v.icons.iter().copied())).collect();
        assert_eq!(all.iter().collect::<HashSet<_>>().len(), Item::ALL.len(), "{all:?}");
        assert_eq!(all.len(), Item::ALL.len(), "each icon exactly once: {all:?}");
    }

    // spec: LAY-002
    #[test]
    fn lay_002_a_stored_order_with_unknown_missing_and_double_ids_is_repaired() {
        assert_eq!(Layout::from_config(&Config::default()), Layout::default(), "nothing stored: the default");
        let d = Layout::default();
        assert_eq!((d.ribbon.len(), d.views.len(), d.views[0].icons.len(), d.views[0].active), (12, 1, 5, p(Backlinks)), "default: all five panels in one view");
        let l = Layout::from_config(&cfg_with(&["mail", "bogus", "files", "mail", "", "new-note", "panel:outline"], &[]));
        assert_eq!(&l.ribbon[..4], [Item::Mail, Item::Files, Item::NewNote, p(Outline)], "the stored order is kept");
        let rest: Vec<Item> = Item::ALL.into_iter().filter(|i| !i.is_panel() && ![Item::Mail, Item::Files, Item::NewNote].contains(i)).collect();
        assert_eq!(&l.ribbon[4..], rest.as_slice(), "icons the list lacks come at the end in default order");
        assert_eq!(l.views, vec![view(&[Backlinks, Outgoing, LocalGraph, Calendar])], "missing panels make a view, in default order");
        let mut c = Config::default();
        l.store(&mut c);
        assert_eq!(Layout::from_config(&c), l, "what is stored comes back as it was");
        for i in Item::ALL {
            assert_eq!(Item::from_id(i.id()), Some(i));
        }
    }

    // spec: LAY-002
    #[test]
    fn lay_002_stored_views_are_repaired_too() {
        // a ribbon item inside a view, a double across views, an unknown id, a view that ends up empty, an active icon not in its view
        let l = Layout::from_config(&cfg_with(&["files"], &[(&["panel:outline", "mail", "panel:outline", "nope"], "panel:calendar"), (&["files", "x"], "files"), (&["panel:backlinks", "panel:outgoing"], "panel:outgoing")]));
        assert_eq!(l.views, vec![view(&[Outline]), View { icons: vec![p(Backlinks), p(Outgoing), p(LocalGraph), p(Calendar)], active: p(Outgoing) }]);
        assert!(l.ribbon.contains(&Item::Mail) && l.ribbon.contains(&Item::Files), "a ribbon item found in a view goes back to the ribbon");
        assert!(!l.ribbon.iter().any(|i| i.is_panel()));
        every_icon_once(&l);
    }

    // spec: LAY-001
    #[test]
    fn moving_in_the_ribbon_counts_the_icons_above_the_drop_point() {
        let mut l = Layout::default();
        l.apply(Item::Files, Dest::Ribbon(3)).unwrap();
        assert_eq!(&l.ribbon[..3], [Item::Search, Item::Tags, Item::Files], "down: the list shrinks above the point");
        l.apply(Item::NewNote, Dest::Ribbon(0)).unwrap();
        assert_eq!(l.ribbon[0], Item::NewNote);
        l.apply(Item::Search, Dest::Ribbon(99)).unwrap();
        assert_eq!(l.ribbon.last(), Some(&Item::Search));
        let before = l.clone();
        l.apply(Item::NewNote, Dest::Ribbon(1)).unwrap();
        assert_eq!(l, before, "dropping an icon right after itself changes nothing");
        assert_eq!(l.ribbon.len(), 12);
    }

    // spec: LAY-005
    #[test]
    fn lay_005_icons_move_between_menubars_and_never_get_copied() {
        let mut l = two_views();
        // right to right: Outline goes to the second view, at the front, and becomes active there
        l.apply(p(Outline), Dest::Bar { view: 1, at: 0 }).unwrap();
        assert_eq!(l.views[0].icons, [p(Backlinks)]);
        assert_eq!((l.views[1].icons.clone(), l.views[1].active), (vec![p(Outline), p(Outgoing), p(LocalGraph), p(Calendar)], p(Outline)));
        // within a view: down moves count the icons as they were
        l.apply(p(Outline), Dest::Bar { view: 1, at: 3 }).unwrap();
        assert_eq!(l.views[1].icons, [p(Outgoing), p(LocalGraph), p(Outline), p(Calendar)]);
        // right to left: a panel becomes a ribbon shortcut, and leaves the view
        l.apply(p(Calendar), Dest::Ribbon(2)).unwrap();
        assert_eq!(l.ribbon[2], p(Calendar));
        assert!(!l.views[1].icons.contains(&p(Calendar)));
        // left to right: the shortcut goes back to a menubar
        l.apply(p(Calendar), Dest::Bar { view: 0, at: 1 }).unwrap();
        assert_eq!(l.views[0].icons, [p(Backlinks), p(Calendar)]);
        assert!(!l.ribbon.contains(&p(Calendar)));
        every_icon_once(&l);
        // a moved active icon: the view shows its first one then
        let mut l = two_views();
        l.apply(p(Backlinks), Dest::Ribbon(0)).unwrap();
        assert_eq!((l.views[0].icons.clone(), l.views[0].active), (vec![p(Outline)], p(Outline)));
    }

    // spec: LAY-006
    #[test]
    fn lay_006_ribbon_items_are_refused_in_a_right_menubar_and_nothing_changes() {
        let mut l = two_views();
        let before = l.clone();
        assert_eq!(l.apply(Item::Mail, Dest::Bar { view: 0, at: 0 }), Err(Refused));
        assert_eq!(l.apply(Item::Files, Dest::NewView { at: 1 }), Err(Refused));
        assert_eq!(l, before);
    }

    // spec: LAY-007
    #[test]
    fn lay_007_the_last_icon_leaving_a_view_removes_the_view() {
        let mut l = Layout { ribbon: vec![], views: vec![view(&[Backlinks]), view(&[Outline, Outgoing]), view(&[Calendar])] };
        // the first view's only icon goes to the third view: the view is gone and the indexes shift
        l.apply(p(Backlinks), Dest::Bar { view: 2, at: 1 }).unwrap();
        assert_eq!(l.views, vec![view(&[Outline, Outgoing]), View { icons: vec![p(Calendar), p(Backlinks)], active: p(Backlinks) }]);
        // an only icon to the ribbon: gone, no view left over
        l.apply(p(Backlinks), Dest::Ribbon(0)).unwrap();
        l.apply(p(Calendar), Dest::Ribbon(0)).unwrap();
        assert_eq!(l.views, vec![view(&[Outline, Outgoing])]);
        // an only icon dropped on its own bar or beside its own view: no change
        let mut one = Layout { ribbon: vec![], views: vec![view(&[Calendar])] };
        let before = one.clone();
        one.apply(p(Calendar), Dest::Bar { view: 0, at: 0 }).unwrap();
        one.apply(p(Calendar), Dest::NewView { at: 1 }).unwrap();
        one.apply(p(Calendar), Dest::NewView { at: 0 }).unwrap();
        assert_eq!(one, before);
        // no view at all is a valid layout, and a drop makes the first one
        let mut none = Layout { ribbon: vec![p(Outline)], views: vec![] };
        none.apply(p(Outline), Dest::NewView { at: 0 }).unwrap();
        assert_eq!((none.ribbon.len(), none.views), (0, vec![view(&[Outline])]));
    }

    // spec: LAY-009
    #[test]
    fn lay_009_a_new_view_is_made_at_the_place_of_the_drop() {
        let mut l = two_views();
        l.apply(p(Calendar), Dest::NewView { at: 1 }).unwrap();
        assert_eq!(l.views, vec![view(&[Backlinks, Outline]), view(&[Calendar]), view(&[Outgoing, LocalGraph])]);
        // from a view that disappears above the place: the place shifts up with the stack
        let mut l = Layout { ribbon: vec![], views: vec![view(&[Backlinks]), view(&[Outline]), view(&[Calendar])] };
        l.apply(p(Backlinks), Dest::NewView { at: 3 }).unwrap();
        assert_eq!(l.views, vec![view(&[Outline]), view(&[Calendar]), view(&[Backlinks])]);
        // from the ribbon
        let mut l = Layout { ribbon: vec![p(Outline)], views: vec![view(&[Backlinks])] };
        l.apply(p(Outline), Dest::NewView { at: 0 }).unwrap();
        assert_eq!((l.ribbon.len(), l.views), (0, vec![view(&[Outline]), view(&[Backlinks])]));
    }

    #[test]
    fn showing_a_panel_makes_it_the_active_one_of_its_view() {
        let mut l = two_views();
        assert!(l.show_panel(Calendar));
        assert_eq!(l.views[1].active, p(Calendar));
        assert_eq!(l.views[0].active, p(Backlinks), "the other view keeps its panel");
        let mut none = Layout { ribbon: vec![p(Outline)], views: vec![] };
        assert!(!none.show_panel(Outline), "a panel in the ribbon is in no view");
    }
}
