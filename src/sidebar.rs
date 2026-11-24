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
    /// the Kanban boards (BOARD-007)
    Boards,
    Social,
    Import,
    Drawing,
    NewNote,
    /// something the right sidebar shows; in the ribbon it is a shortcut to that
    Panel(RightPanel),
}

impl Item {
    /// All items in their default order: the ribbon's, then the panels'.
    pub const ALL: [Item; 20] = [
        Item::Files,
        Item::Search,
        Item::Tags,
        Item::Switcher,
        Item::Graph,
        Item::Calendar,
        Item::Mail,
        Item::Tasks,
        Item::Boards,
        Item::Social,
        Item::Import,
        Item::Drawing,
        Item::NewNote,
        Item::Panel(RightPanel::Backlinks),
        Item::Panel(RightPanel::Outgoing),
        Item::Panel(RightPanel::Outline),
        Item::Panel(RightPanel::LocalGraph),
        Item::Panel(RightPanel::Calendar),
        Item::Panel(RightPanel::History),
        Item::Panel(RightPanel::Thread),
    ];

    /// The panels, in the default order of the right sidebar's menubar.
    pub fn is_panel(self) -> bool {
        matches!(self, Item::Panel(_))
    }

    /// Files, Search and Tags: the views of the left sidebar, which can be in the ribbon, in a
    /// right view or in both (LAY-020, ADR-0033).
    pub fn is_movable(self) -> bool {
        matches!(self, Item::Files | Item::Search | Item::Tags)
    }

    /// Can a right menubar take this icon: a panel or a movable view (LAY-006, LAY-020).
    pub fn fits_right(self) -> bool {
        self.is_panel() || self.is_movable()
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
            Item::Boards => "boards",
            Item::Social => "social",
            Item::Import => "import",
            Item::Drawing => "drawing",
            Item::NewNote => "new-note",
            Item::Panel(RightPanel::Backlinks) => "panel:backlinks",
            Item::Panel(RightPanel::Outgoing) => "panel:outgoing",
            Item::Panel(RightPanel::Outline) => "panel:outline",
            Item::Panel(RightPanel::LocalGraph) => "panel:graph",
            Item::Panel(RightPanel::Calendar) => "panel:calendar",
            Item::Panel(RightPanel::History) => "panel:history",
            Item::Panel(RightPanel::Thread) => "panel:thread",
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
            Item::Boards => Icon::Boards,
            Item::Social => Icon::Social,
            Item::Import => Icon::Import,
            Item::Drawing => Icon::Drawing,
            Item::NewNote => Icon::NewNote,
            Item::Panel(RightPanel::Backlinks) => Icon::Backlinks,
            Item::Panel(RightPanel::Outgoing) => Icon::Outgoing,
            Item::Panel(RightPanel::Outline) => Icon::Outline,
            Item::Panel(RightPanel::LocalGraph) => Icon::LocalGraph,
            Item::Panel(RightPanel::Calendar) => Icon::CalendarPanel,
            Item::Panel(RightPanel::History) => Icon::History,
            Item::Panel(RightPanel::Thread) => Icon::Thread,
        }
    }

    /// The name in the context menu of an icon (LAY-013).
    pub fn name(self) -> &'static str {
        match self {
            Item::Files => "Files",
            Item::Search => "Search",
            Item::Tags => "Tags",
            Item::Switcher => "Quick switcher",
            Item::Graph => "Graph",
            Item::Calendar => "Calendar",
            Item::Mail => "Mail",
            Item::Tasks => "Tasks",
            Item::Boards => "Boards",
            Item::Social => "Feeds",
            Item::Import => "Import",
            Item::Drawing => "New drawing",
            Item::NewNote => "New note",
            Item::Panel(RightPanel::Backlinks) => "Backlinks panel",
            Item::Panel(RightPanel::Outgoing) => "Outgoing links panel",
            Item::Panel(RightPanel::Outline) => "Outline panel",
            Item::Panel(RightPanel::LocalGraph) => "Local graph panel",
            Item::Panel(RightPanel::Calendar) => "Calendar panel",
            Item::Panel(RightPanel::History) => "History panel",
            Item::Panel(RightPanel::Thread) => "Thread panel",
        }
    }

    pub fn tip(self) -> &'static str {
        match self {
            Item::Files => "Files",
            Item::Search => "Search notes, mail and posts (Ctrl+Shift+F)",
            Item::Tags => "Tags",
            Item::Switcher => "Quick switcher (Ctrl+O)",
            Item::Graph => "Graph of all notes",
            Item::Calendar => "Calendar",
            Item::Mail => "Mail (Ctrl+Shift+M)",
            Item::Tasks => "To-do list across all notes",
            Item::Boards => "Kanban boards (Space b)",
            Item::Social => "Feeds: read Mastodon and Bluesky and save posts to the vault",
            Item::Import => "Import e-mails, events and files (or drop them on the window)",
            Item::Drawing => "New drawing",
            Item::NewNote => "New note (Ctrl+N)",
            Item::Panel(RightPanel::Backlinks) => "Backlinks and unlinked mentions",
            Item::Panel(RightPanel::Outgoing) => "Outgoing links",
            Item::Panel(RightPanel::Outline) => "Outline of the note",
            Item::Panel(RightPanel::LocalGraph) => "Graph around the note",
            Item::Panel(RightPanel::Calendar) => "Calendar",
            Item::Panel(RightPanel::History) => "File history of the note",
            Item::Panel(RightPanel::Thread) => "The thread of the note: its mails, oldest first (Ctrl+Shift+T)",
        }
    }
}

/// The share of the height every view starts with (LAY-012).
pub const EQUAL: u32 = 1000;
/// What a stored weight is kept between.
const MIN_WEIGHT: u32 = 50;
const MAX_WEIGHT: u32 = 100_000;
/// The smallest height of a view of the right sidebar, in points (LAY-012).
pub const MIN_VIEW_H: f32 = 80.0;

/// One view of the right sidebar: a menubar of panel icons and the panel of the active one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct View {
    pub icons: Vec<Item>,
    pub active: Item,
    /// its share of the height of the sidebar, against the shares of the other views (LAY-012)
    pub weight: u32,
}

impl View {
    pub fn new(icons: Vec<Item>, active: Item) -> View {
        View { icons, active, weight: EQUAL }
    }
}

/// The border between two neighbouring views was dragged so that the upper one is `top_h` high
/// (in points) of a sidebar `full` high; the weights of the two, `a` (upper) and `b`, change and
/// keep their sum, so the other views do not move. `all` is the sum of all weights. Neither view
/// gets smaller than `MIN_VIEW_H` (LAY-012).
pub fn drag_border(a: u32, b: u32, all: u32, full: f32, top_h: f32) -> (u32, u32) {
    let pair = (a + b) as f32 / all as f32 * full;
    if pair < 2.0 * MIN_VIEW_H {
        return (a, b);
    }
    let top = top_h.clamp(MIN_VIEW_H, pair - MIN_VIEW_H);
    let wa = ((top / pair) * (a + b) as f32).round() as u32;
    (wa, a + b - wa)
}

/// Where an icon is dropped. Counts are "this many icons (views) above or before it", in the
/// layout as it is before the move.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dest {
    Ribbon(usize),
    Bar { view: usize, at: usize },
    NewView { at: usize },
}

impl Dest {
    /// The side of the window the place is on.
    pub fn side(self) -> Side {
        match self {
            Dest::Ribbon(_) => Side::Left,
            Dest::Bar { .. } | Dest::NewView { .. } => Side::Right,
        }
    }
}

/// The left sidebar (the ribbon) or the right one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Side {
    #[default]
    Left,
    Right,
}

/// What a drop of a movable view on the other side does (LAY-021): it leaves the side it came
/// from, or stays there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Move,
    Copy,
}

/// A right menubar takes panel icons and the movable views only (LAY-006, LAY-020).
#[derive(Debug, PartialEq, Eq)]
pub struct Refused;

/// The icons of the window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    pub ribbon: Vec<Item>,
    pub views: Vec<View>,
    /// icons the user removed; they are in no menubar until restored (LAY-013)
    pub hidden: Vec<Item>,
}

impl Default for Layout {
    fn default() -> Self {
        let panels: Vec<Item> = Item::ALL.into_iter().filter(|i| i.is_panel()).collect();
        Layout { ribbon: Item::ALL.into_iter().filter(|i| !i.is_panel()).collect(), views: vec![View::new(panels.clone(), panels[0])], hidden: vec![] }
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
        // a movable view may be on both sides (a copy); every other icon is in one place only
        let mut seen = HashSet::new();
        let mut placed = HashSet::new();
        let mut views: Vec<View> = vec![];
        for v in &cfg.sidebars.right {
            let icons: Vec<Item> = v.icons.iter().filter_map(|id| Item::from_id(id)).filter(|i| i.fits_right() && seen.insert((*i, i.is_movable()))).inspect(|i| _ = placed.insert(*i)).collect();
            if let Some(first) = icons.first().copied() {
                let active = Item::from_id(&v.active).filter(|a| icons.contains(a)).unwrap_or(first);
                let weight = if v.weight == 0 { EQUAL } else { v.weight.clamp(MIN_WEIGHT, MAX_WEIGHT) };
                views.push(View { icons, active, weight });
            }
        }
        let mut ribbon: Vec<Item> = cfg.sidebars.ribbon.iter().filter_map(|id| Item::from_id(id)).filter(|i| seen.insert((*i, false))).inspect(|i| _ = placed.insert(*i)).collect();
        // an icon that is shown is not hidden; a hidden one does not come back as a missing one
        let hidden: Vec<Item> = cfg.sidebars.hidden.iter().filter_map(|id| Item::from_id(id)).filter(|i| placed.insert(*i)).collect();
        for item in Item::ALL.into_iter().filter(|i| placed.insert(*i)) {
            match (item.is_panel(), views.last_mut()) {
                (false, _) => ribbon.push(item),
                (true, Some(v)) => v.icons.push(item),
                (true, None) => views.push(View::new(vec![item], item)),
            }
        }
        Layout { ribbon, views, hidden }
    }

    /// Write the layout into the settings.
    pub fn store(&self, cfg: &mut Config) {
        let ids = |v: &[Item]| v.iter().map(|i| i.id().to_string()).collect();
        cfg.sidebars = SidebarCfg { ribbon: ids(&self.ribbon), right: self.views.iter().map(|v| RightViewCfg { icons: ids(&v.icons), active: v.active.id().to_string(), weight: v.weight }).collect(), hidden: ids(&self.hidden) };
    }

    fn in_ribbon(&self, item: Item) -> Option<usize> {
        self.ribbon.iter().position(|i| *i == item)
    }

    fn in_view(&self, item: Item) -> Option<(usize, usize)> {
        self.views.iter().enumerate().find_map(|(v, w)| w.icons.iter().position(|i| *i == item).map(|p| (v, p)))
    }

    /// Is the icon in a view of the right sidebar.
    pub fn on_right(&self, item: Item) -> bool {
        self.in_view(item).is_some()
    }

    /// Is the icon in the ribbon.
    pub fn on_left(&self, item: Item) -> bool {
        self.in_ribbon(item).is_some()
    }

    /// Move an icon to a place (LAY-005). A right menubar or a new view takes panel icons and the
    /// movable views only (LAY-006, LAY-020). A view that loses its last icon is removed
    /// (LAY-007); an icon that arrives in a view becomes its active one. `from` is the side the
    /// drag started at, which matters for a movable view that is on both sides; dropping one on
    /// the other side takes it from `from` with `Mode::Move` and leaves it there with
    /// `Mode::Copy`, and it is not added again where it already is (LAY-021, LAY-022).
    pub fn apply(&mut self, item: Item, dest: Dest, from: Side, mode: Mode) -> Result<(), Refused> {
        if !item.fits_right() && dest.side() == Side::Right {
            return Err(Refused);
        }
        let (mut from_ribbon, mut from_view) = (self.in_ribbon(item), self.in_view(item));
        let cross = item.is_movable() && from != dest.side();
        if item.is_movable() {
            // only the side the drag started at is the source
            match from {
                Side::Left => from_view = None,
                Side::Right => from_ribbon = None,
            }
        }
        let already = cross && (if dest.side() == Side::Left { self.on_left(item) } else { self.on_right(item) });
        let keep_source = cross && mode == Mode::Copy;
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
        if keep_source && !already {
            from_ribbon = None;
            from_view = None;
        } else if keep_source {
            return Ok(());
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
        if already {
            return Ok(());
        }
        match dest {
            Dest::Ribbon(at) => self.ribbon.insert(at.min(self.ribbon.len()), item),
            Dest::Bar { view, at } => {
                let v = &mut self.views[view];
                v.icons.insert(at.min(v.icons.len()), item);
                v.active = item;
            }
            Dest::NewView { at } => self.views.insert(at.min(self.views.len()), View::new(vec![item], item)),
        }
        Ok(())
    }

    /// Remove an icon from its menubar (on this `side`, for a movable view that is on both). It
    /// is hidden (LAY-013) when it is then in no menubar. A view that loses its last icon is
    /// removed (LAY-007). False: the icon is not in a menubar of that side.
    pub fn hide(&mut self, item: Item, side: Side) -> bool {
        let (ribbon, view) = (self.in_ribbon(item), self.in_view(item));
        let (ribbon, view) = if item.is_movable() { (ribbon.filter(|_| side == Side::Left), view.filter(|_| side == Side::Right)) } else { (ribbon, view) };
        if let Some(p) = ribbon {
            self.ribbon.remove(p);
        } else if let Some((v, p)) = view {
            self.views[v].icons.remove(p);
            if self.views[v].icons.is_empty() {
                self.views.remove(v);
            } else if self.views[v].active == item {
                self.views[v].active = self.views[v].icons[0];
            }
        } else {
            return false;
        }
        if !self.on_left(item) && !self.on_right(item) {
            self.hidden.push(item);
        }
        true
    }

    /// Bring a hidden icon back (LAY-013): a ribbon item to the end of the ribbon, a panel to the
    /// end of the last view (a new view if there is none). False: it was not hidden.
    pub fn restore(&mut self, item: Item) -> bool {
        let Some(p) = self.hidden.iter().position(|i| *i == item) else { return false };
        self.hidden.remove(p);
        match (item.is_panel(), self.views.last_mut()) {
            (false, _) => self.ribbon.push(item),
            (true, Some(v)) => v.icons.push(item),
            (true, None) => self.views.push(View::new(vec![item], item)),
        }
        true
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
        View::new(icons.iter().map(|x| p(*x)).collect(), p(icons[0]))
    }

    /// a plain move of an icon that is in one place
    fn mv(l: &mut Layout, item: Item, dest: Dest) -> Result<(), Refused> {
        let from = if l.on_right(item) { Side::Right } else { Side::Left };
        l.apply(item, dest, from, Mode::Move)
    }

    fn two_views() -> Layout {
        Layout { ribbon: Layout::default().ribbon, views: vec![view(&[Backlinks, Outline]), view(&[Outgoing, LocalGraph, Calendar, History, Thread])], hidden: vec![] }
    }

    fn cfg_with(ribbon: &[&str], views: &[(&[&str], &str)]) -> Config {
        let ids = |v: &[&str]| v.iter().map(|s| s.to_string()).collect();
        Config { sidebars: SidebarCfg { ribbon: ids(ribbon), right: views.iter().map(|(i, a)| RightViewCfg { icons: ids(i), active: a.to_string(), weight: 0 }).collect(), hidden: vec![] }, ..Config::default() }
    }

    fn every_icon_once(l: &Layout) {
        let all: Vec<Item> = l.ribbon.iter().copied().chain(l.views.iter().flat_map(|v| v.icons.iter().copied())).collect();
        let all: Vec<Item> = all.into_iter().chain(l.hidden.iter().copied()).collect();
        assert_eq!(all.iter().collect::<HashSet<_>>().len(), Item::ALL.len(), "{all:?}");
        assert_eq!(all.len(), Item::ALL.len(), "each icon exactly once: {all:?}");
    }

    // spec: LAY-002
    #[test]
    fn lay_002_a_stored_order_with_unknown_missing_and_double_ids_is_repaired() {
        assert_eq!(Layout::from_config(&Config::default()), Layout::default(), "nothing stored: the default");
        let d = Layout::default();
        assert_eq!((d.ribbon.len(), d.views.len(), d.views[0].icons.len(), d.views[0].active), (13, 1, 7, p(Backlinks)), "default: all seven panels in one view");
        let l = Layout::from_config(&cfg_with(&["mail", "bogus", "files", "mail", "", "new-note", "panel:outline"], &[]));
        assert_eq!(&l.ribbon[..4], [Item::Mail, Item::Files, Item::NewNote, p(Outline)], "the stored order is kept");
        let rest: Vec<Item> = Item::ALL.into_iter().filter(|i| !i.is_panel() && ![Item::Mail, Item::Files, Item::NewNote].contains(i)).collect();
        assert_eq!(&l.ribbon[4..], rest.as_slice(), "icons the list lacks come at the end in default order");
        assert_eq!(l.views, vec![view(&[Backlinks, Outgoing, LocalGraph, Calendar, History, Thread])], "missing panels make a view, in default order");
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
        let l = Layout::from_config(&cfg_with(&["files"], &[(&["panel:outline", "mail", "panel:outline", "nope"], "panel:calendar"), (&["mail", "x"], "mail"), (&["panel:backlinks", "panel:outgoing"], "panel:outgoing")]));
        assert_eq!(l.views, vec![view(&[Outline]), View::new(vec![p(Backlinks), p(Outgoing), p(LocalGraph), p(Calendar), p(History), p(Thread)], p(Outgoing))]);
        assert!(l.ribbon.contains(&Item::Mail) && l.ribbon.contains(&Item::Files), "a ribbon item found in a view goes back to the ribbon");
        assert!(!l.ribbon.iter().any(|i| i.is_panel()));
        every_icon_once(&l);
    }

    // spec: LAY-001
    #[test]
    fn moving_in_the_ribbon_counts_the_icons_above_the_drop_point() {
        let mut l = Layout::default();
        mv(&mut l, Item::Files, Dest::Ribbon(3)).unwrap();
        assert_eq!(&l.ribbon[..3], [Item::Search, Item::Tags, Item::Files], "down: the list shrinks above the point");
        mv(&mut l, Item::NewNote, Dest::Ribbon(0)).unwrap();
        assert_eq!(l.ribbon[0], Item::NewNote);
        mv(&mut l, Item::Search, Dest::Ribbon(99)).unwrap();
        assert_eq!(l.ribbon.last(), Some(&Item::Search));
        let before = l.clone();
        mv(&mut l, Item::NewNote, Dest::Ribbon(1)).unwrap();
        assert_eq!(l, before, "dropping an icon right after itself changes nothing");
        assert_eq!(l.ribbon.len(), 13);
    }

    // spec: LAY-005
    #[test]
    fn lay_005_icons_move_between_menubars_and_never_get_copied() {
        let mut l = two_views();
        // right to right: Outline goes to the second view, at the front, and becomes active there
        mv(&mut l, p(Outline), Dest::Bar { view: 1, at: 0 }).unwrap();
        assert_eq!(l.views[0].icons, [p(Backlinks)]);
        assert_eq!((l.views[1].icons.clone(), l.views[1].active), (vec![p(Outline), p(Outgoing), p(LocalGraph), p(Calendar), p(History), p(Thread)], p(Outline)));
        // within a view: down moves count the icons as they were
        mv(&mut l, p(Outline), Dest::Bar { view: 1, at: 3 }).unwrap();
        assert_eq!(l.views[1].icons, [p(Outgoing), p(LocalGraph), p(Outline), p(Calendar), p(History), p(Thread)]);
        // right to left: a panel becomes a ribbon shortcut, and leaves the view
        mv(&mut l, p(Calendar), Dest::Ribbon(2)).unwrap();
        assert_eq!(l.ribbon[2], p(Calendar));
        assert!(!l.views[1].icons.contains(&p(Calendar)));
        // left to right: the shortcut goes back to a menubar
        mv(&mut l, p(Calendar), Dest::Bar { view: 0, at: 1 }).unwrap();
        assert_eq!(l.views[0].icons, [p(Backlinks), p(Calendar)]);
        assert!(!l.ribbon.contains(&p(Calendar)));
        every_icon_once(&l);
        // a moved active icon: the view shows its first one then
        let mut l = two_views();
        mv(&mut l, p(Backlinks), Dest::Ribbon(0)).unwrap();
        assert_eq!((l.views[0].icons.clone(), l.views[0].active), (vec![p(Outline)], p(Outline)));
    }

    // spec: LAY-006
    #[test]
    fn lay_006_ribbon_items_are_refused_in_a_right_menubar_and_nothing_changes() {
        let mut l = two_views();
        let before = l.clone();
        assert_eq!(mv(&mut l, Item::Mail, Dest::Bar { view: 0, at: 0 }), Err(Refused));
        assert_eq!(mv(&mut l, Item::Switcher, Dest::NewView { at: 1 }), Err(Refused));
        assert_eq!(l, before);
    }

    // spec: LAY-007
    #[test]
    fn lay_007_the_last_icon_leaving_a_view_removes_the_view() {
        let mut l = Layout { ribbon: vec![], views: vec![view(&[Backlinks]), view(&[Outline, Outgoing]), view(&[Calendar])], hidden: vec![] };
        // the first view's only icon goes to the third view: the view is gone and the indexes shift
        mv(&mut l, p(Backlinks), Dest::Bar { view: 2, at: 1 }).unwrap();
        assert_eq!(l.views, vec![view(&[Outline, Outgoing]), View::new(vec![p(Calendar), p(Backlinks)], p(Backlinks))]);
        // an only icon to the ribbon: gone, no view left over
        mv(&mut l, p(Backlinks), Dest::Ribbon(0)).unwrap();
        mv(&mut l, p(Calendar), Dest::Ribbon(0)).unwrap();
        assert_eq!(l.views, vec![view(&[Outline, Outgoing])]);
        // an only icon dropped on its own bar or beside its own view: no change
        let mut one = Layout { ribbon: vec![], views: vec![view(&[Calendar])], hidden: vec![] };
        let before = one.clone();
        mv(&mut one, p(Calendar), Dest::Bar { view: 0, at: 0 }).unwrap();
        mv(&mut one, p(Calendar), Dest::NewView { at: 1 }).unwrap();
        mv(&mut one, p(Calendar), Dest::NewView { at: 0 }).unwrap();
        assert_eq!(one, before);
        // no view at all is a valid layout, and a drop makes the first one
        let mut none = Layout { ribbon: vec![p(Outline)], views: vec![], hidden: vec![] };
        mv(&mut none, p(Outline), Dest::NewView { at: 0 }).unwrap();
        assert_eq!((none.ribbon.len(), none.views), (0, vec![view(&[Outline])]));
    }

    // spec: LAY-009
    #[test]
    fn lay_009_a_new_view_is_made_at_the_place_of_the_drop() {
        let mut l = two_views();
        mv(&mut l, p(Calendar), Dest::NewView { at: 1 }).unwrap();
        assert_eq!(l.views, vec![view(&[Backlinks, Outline]), view(&[Calendar]), view(&[Outgoing, LocalGraph, History, Thread])]);
        // from a view that disappears above the place: the place shifts up with the stack
        let mut l = Layout { ribbon: vec![], views: vec![view(&[Backlinks]), view(&[Outline]), view(&[Calendar])], hidden: vec![] };
        mv(&mut l, p(Backlinks), Dest::NewView { at: 3 }).unwrap();
        assert_eq!(l.views, vec![view(&[Outline]), view(&[Calendar]), view(&[Backlinks])]);
        // from the ribbon
        let mut l = Layout { ribbon: vec![p(Outline)], views: vec![view(&[Backlinks])], hidden: vec![] };
        mv(&mut l, p(Outline), Dest::NewView { at: 0 }).unwrap();
        assert_eq!((l.ribbon.len(), l.views), (0, vec![view(&[Outline]), view(&[Backlinks])]));
    }

    #[test]
    fn showing_a_panel_makes_it_the_active_one_of_its_view() {
        let mut l = two_views();
        assert!(l.show_panel(Calendar));
        assert_eq!(l.views[1].active, p(Calendar));
        assert_eq!(l.views[0].active, p(Backlinks), "the other view keeps its panel");
        let mut none = Layout { ribbon: vec![p(Outline)], views: vec![], hidden: vec![] };
        assert!(!none.show_panel(Outline), "a panel in the ribbon is in no view");
    }

    // spec: LAY-012
    #[test]
    fn lay_012_dragging_a_border_moves_height_between_two_views_and_keeps_a_minimum() {
        // three equal views in 900 points: the first border goes 60 points down
        let all = 3 * EQUAL;
        let (a, b) = drag_border(EQUAL, EQUAL, all, 900.0, 300.0 + 60.0);
        assert_eq!(a + b, 2 * EQUAL, "the pair keeps its sum, so the third view does not move");
        assert!((a as f32 / all as f32 * 900.0 - 360.0).abs() < 1.0);
        // never below the minimum, on either side
        let (a, b) = drag_border(EQUAL, EQUAL, all, 900.0, -50.0);
        assert!((a as f32 / all as f32 * 900.0 - MIN_VIEW_H).abs() < 1.0, "{a}");
        let (a, b2) = drag_border(EQUAL, EQUAL, all, 900.0, 5000.0);
        assert!((b2 as f32 / all as f32 * 900.0 - MIN_VIEW_H).abs() < 1.0, "{b2}");
        assert_eq!((a + b2, b > 0), (2 * EQUAL, true));
        // two views too small for the minimum stay as they are
        assert_eq!(drag_border(EQUAL, EQUAL, 2 * EQUAL, 100.0, 70.0), (EQUAL, EQUAL));
    }

    // spec: LAY-012
    #[test]
    fn lay_012_stored_weights_come_back_and_a_layout_without_them_is_equal() {
        let mut cfg = cfg_with(&[], &[(&["panel:backlinks"], "panel:backlinks"), (&["panel:outline"], "panel:outline")]);
        let l = Layout::from_config(&cfg);
        assert_eq!(l.views.iter().map(|v| v.weight).collect::<Vec<_>>(), [EQUAL, EQUAL], "no weight stored: equal heights");
        cfg.sidebars.right[0].weight = 3000;
        cfg.sidebars.right[1].weight = 1;
        let mut l = Layout::from_config(&cfg);
        assert_eq!(l.views.iter().map(|v| v.weight).collect::<Vec<_>>(), [3000, MIN_WEIGHT], "a silly weight is brought into range");
        l.views[1].weight = 1500;
        let mut stored = Config::default();
        l.store(&mut stored);
        assert_eq!(Layout::from_config(&stored), l);
        // an old settings file has no weights and no hidden icons
        let (old, _) = crate::vault_state::parse::<Config>(br#"{"sidebars": {"ribbon": ["mail"], "right": [{"icons": ["panel:outline"], "active": "panel:outline"}]}}"#).unwrap();
        assert_eq!(Layout::from_config(&old).views[0].weight, EQUAL);
    }

    // spec: LAY-013
    #[test]
    fn lay_013_hiding_and_restoring_icons_keeps_each_icon_in_exactly_one_place() {
        let mut l = two_views();
        assert!(l.hide(Item::Mail, Side::Left));
        assert!(l.hide(p(Backlinks), Side::Right));
        assert!(!l.hide(Item::Mail, Side::Left), "an icon that is hidden cannot be hidden again");
        assert_eq!(l.hidden, [Item::Mail, p(Backlinks)]);
        assert_eq!((l.views[0].icons.clone(), l.views[0].active), (vec![p(Outline)], p(Outline)));
        every_icon_once(&l);
        // the last icon of a view takes the view with it
        assert!(l.hide(p(Outline), Side::Right));
        assert_eq!(l.views, vec![view(&[Outgoing, LocalGraph, Calendar, History, Thread])]);
        // what is hidden stays hidden when the layout is stored and loaded
        let mut c = Config::default();
        l.store(&mut c);
        assert_eq!(Layout::from_config(&c), l);
        every_icon_once(&Layout::from_config(&c));
        // restoring: a ribbon item at the end of the ribbon, a panel at the end of the last view
        assert!(l.restore(Item::Mail));
        assert_eq!(l.ribbon.last(), Some(&Item::Mail));
        assert!(l.restore(p(Backlinks)));
        assert_eq!(l.views[0].icons.last(), Some(&p(Backlinks)));
        assert!(!l.restore(Item::Mail), "it is not hidden any more");
        // with no view a restored panel makes one
        let mut none = Layout { ribbon: vec![], views: vec![], hidden: vec![p(Outline)] };
        assert!(none.restore(p(Outline)));
        assert_eq!(none.views, vec![view(&[Outline])]);
        // an icon that is in a menubar is not hidden, whatever the file says
        let l = Layout::from_config(&{
            let mut c = cfg_with(&["mail"], &[]);
            c.sidebars.hidden = vec!["mail".into(), "files".into(), "nope".into()];
            c
        });
        assert_eq!(l.hidden, [Item::Files]);
        assert!(l.ribbon.contains(&Item::Mail) && !l.ribbon.contains(&Item::Files));
    }

    // spec: LAY-020
    #[test]
    fn lay_020_files_search_and_tags_fit_a_right_menubar_and_the_other_ribbon_items_do_not() {
        for item in [Item::Files, Item::Search, Item::Tags] {
            assert!(item.is_movable() && item.fits_right() && !item.is_panel(), "{item:?}");
        }
        for item in [Item::Mail, Item::Calendar, Item::Graph, Item::Tasks, Item::Social, Item::Switcher, Item::Import, Item::Drawing, Item::NewNote] {
            assert!(!item.fits_right(), "{item:?} is a main view or a command");
            let mut l = two_views();
            let before = l.clone();
            assert_eq!(l.apply(item, Dest::Bar { view: 0, at: 0 }, Side::Left, Mode::Copy), Err(Refused));
            assert_eq!(l, before);
        }
    }

    // spec: LAY-021
    #[test]
    fn lay_021_moving_or_copying_a_view_from_the_ribbon_to_the_right_sidebar() {
        // move: it leaves the ribbon and lands where the drop says, as the active icon
        let mut l = two_views();
        l.apply(Item::Files, Dest::Bar { view: 1, at: 1 }, Side::Left, Mode::Move).unwrap();
        assert!(!l.on_left(Item::Files) && l.views[1].icons[1] == Item::Files && l.views[1].active == Item::Files);
        every_icon_once(&l);
        // copy: it stays in the ribbon at its place and is also in the view
        let mut l = two_views();
        l.apply(Item::Search, Dest::Bar { view: 0, at: 0 }, Side::Left, Mode::Copy).unwrap();
        assert_eq!((l.ribbon.clone(), l.views[0].icons[0]), (Layout::default().ribbon, Item::Search));
        // a copy to a new view, at a place of the stack
        l.apply(Item::Tags, Dest::NewView { at: 1 }, Side::Left, Mode::Copy).unwrap();
        assert_eq!(l.views[1], View::new(vec![Item::Tags], Item::Tags));
        assert!(l.on_left(Item::Tags));
        // it is in the view once: a second copy into another view takes it from there instead (it is not added twice)
        l.apply(Item::Search, Dest::Bar { view: 1, at: 0 }, Side::Left, Mode::Copy).unwrap();
        assert_eq!(l.views.iter().flat_map(|v| v.icons.iter()).filter(|i| **i == Item::Search).count(), 1, "a view is in one place per side");
    }

    // spec: LAY-022
    #[test]
    fn lay_022_moving_or_copying_a_view_from_the_right_sidebar_to_the_ribbon() {
        // a view that was moved away comes back to the place of the drop
        let mut l = two_views();
        l.apply(Item::Files, Dest::Bar { view: 0, at: 0 }, Side::Left, Mode::Move).unwrap();
        l.apply(Item::Files, Dest::Ribbon(2), Side::Right, Mode::Move).unwrap();
        assert_eq!(l.ribbon[2], Item::Files);
        assert!(!l.on_right(Item::Files));
        assert_eq!(l.views, two_views().views);
        // the last icon of a view going back removes the view
        let mut l = Layout { ribbon: Layout::default().ribbon.into_iter().filter(|i| *i != Item::Tags).collect(), views: vec![view(&[Backlinks]), View::new(vec![Item::Tags], Item::Tags)], hidden: vec![] };
        l.apply(Item::Tags, Dest::Ribbon(0), Side::Right, Mode::Move).unwrap();
        assert_eq!((l.views.len(), l.ribbon[0]), (1, Item::Tags));
        // a copy dragged to the ribbon, where the view already is: the copy is just taken out of the view (Move), or nothing happens (Copy)
        let mut l = two_views();
        l.apply(Item::Files, Dest::Bar { view: 0, at: 0 }, Side::Left, Mode::Copy).unwrap();
        let both = l.clone();
        l.apply(Item::Files, Dest::Ribbon(5), Side::Right, Mode::Copy).unwrap();
        assert_eq!(l, both);
        l.apply(Item::Files, Dest::Ribbon(5), Side::Right, Mode::Move).unwrap();
        assert_eq!(l, two_views());
        // a copy to the ribbon from a view: both stay
        let mut l = Layout { ribbon: Layout::default().ribbon.into_iter().filter(|i| *i != Item::Search).collect(), views: vec![view(&[Backlinks]), View::new(vec![Item::Search], Item::Search)], hidden: vec![] };
        l.apply(Item::Search, Dest::Ribbon(1), Side::Right, Mode::Copy).unwrap();
        assert_eq!((l.ribbon[1], l.on_right(Item::Search)), (Item::Search, true));
    }

    // spec: LAY-023
    #[test]
    fn lay_023_the_order_in_the_right_stack_follows_the_drop() {
        let mut l = two_views();
        l.apply(Item::Files, Dest::Bar { view: 0, at: 1 }, Side::Left, Mode::Move).unwrap();
        l.apply(Item::Search, Dest::Bar { view: 0, at: 0 }, Side::Left, Mode::Move).unwrap();
        assert_eq!(l.views[0].icons, [Item::Search, p(Backlinks), Item::Files, p(Outline)]);
        // within the view the icon is moved, never copied (it is on this side once)
        mv(&mut l, Item::Search, Dest::Bar { view: 0, at: 4 }).unwrap();
        assert_eq!(l.views[0].icons, [p(Backlinks), Item::Files, p(Outline), Item::Search]);
    }

    // spec: LAY-026
    #[test]
    fn lay_026_the_two_sides_of_a_movable_view_are_stored_and_an_old_layout_loads_as_before() {
        // an old layout has Files, Search and Tags in the ribbon only
        let l = Layout::from_config(&cfg_with(&["files", "search", "tags", "mail"], &[(&["panel:outline"], "panel:outline")]));
        assert_eq!(&l.ribbon[..4], [Item::Files, Item::Search, Item::Tags, Item::Mail]);
        assert!(!l.on_right(Item::Files));
        // a copy is in both lists, a moved view only in the view; a view in no list comes back to the ribbon
        let l = Layout::from_config(&cfg_with(&["files", "mail"], &[(&["panel:outline", "files", "search"], "search")]));
        assert!(l.on_left(Item::Files) && l.on_right(Item::Files), "a copy stays");
        assert!(!l.on_left(Item::Search) && l.on_right(Item::Search), "a moved view stays moved");
        assert!(l.on_left(Item::Tags), "a view that is in no list is in the ribbon");
        assert_eq!(l.views[0].active, Item::Search);
        let mut c = Config::default();
        l.store(&mut c);
        assert_eq!(&c.sidebars.right[0].icons[..3], ["panel:outline", "files", "search"]);
        assert_eq!(Layout::from_config(&c), l, "what is stored comes back");
        // doubles on one side go, other ribbon items in a view go (back to the ribbon), a hidden one stays hidden
        let mut c = cfg_with(&["files", "files"], &[(&["search", "mail", "search"], "mail"), (&["search", "tags"], "tags")]);
        c.sidebars.hidden = vec!["tags".into(), "files".into()];
        let l = Layout::from_config(&c);
        assert_eq!((l.views.len(), l.views[0].icons.clone(), l.views[0].active), (2, vec![Item::Search], Item::Search));
        assert_eq!(l.views[1].icons[0], Item::Tags);
        assert!(l.ribbon.contains(&Item::Mail) && l.ribbon.iter().filter(|i| **i == Item::Files).count() == 1);
        assert!(l.hidden.is_empty(), "both are shown, so neither is hidden: {:?}", l.hidden);
    }

    // spec: LAY-027
    #[test]
    fn lay_027_removing_a_movable_view_takes_it_from_one_side_and_hides_it_when_it_is_nowhere() {
        let mut l = two_views();
        l.apply(Item::Files, Dest::Bar { view: 0, at: 0 }, Side::Left, Mode::Copy).unwrap();
        assert!(l.hide(Item::Files, Side::Right));
        assert!(l.on_left(Item::Files) && l.hidden.is_empty(), "still in the ribbon: not hidden");
        assert!(!l.hide(Item::Files, Side::Right), "it is not in a view any more");
        assert!(l.hide(Item::Files, Side::Left));
        assert_eq!(l.hidden, [Item::Files]);
        assert!(l.restore(Item::Files));
        assert_eq!(l.ribbon.last(), Some(&Item::Files));
        // a view only on the right, removed there: hidden
        let mut l = two_views();
        l.apply(Item::Tags, Dest::Bar { view: 1, at: 0 }, Side::Left, Mode::Move).unwrap();
        assert!(l.hide(Item::Tags, Side::Right));
        assert_eq!(l.hidden, [Item::Tags]);
        every_icon_once(&l);
    }

    // spec: LAY-015
    #[test]
    fn lay_015_every_icon_has_its_own_glyph() {
        let glyphs: HashSet<crate::layout::Icon> = Item::ALL.into_iter().map(|i| i.icon()).collect();
        assert_eq!(glyphs.len(), Item::ALL.len(), "two icons share a glyph");
    }
}
