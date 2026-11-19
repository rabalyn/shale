//! Dataflow diagrams recorded from real runs (DEV-026, #409). Test code only: a flow's steps are
//! `tracing` spans named `flow.<flow>.<step>` with one static field, `peer` (the other side of the
//! step). [`record`] runs a closure under a recorder that keeps the spans as a tree of names and
//! peers (no times, ids or threads), makes the tree canonical (see [`canonical`]), and
//! [`render_mermaid`] turns it into a sequence diagram, which [`verify`] compares with the block
//! between the generated-block markers of `docs/dataflow/<flow>.md`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::{Arc, Mutex};
use tracing::field::{Field, Visit};
use tracing::span::Attributes;
use tracing::{Id, Subscriber};
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::{Layer, Registry};

/// One recorded span with the spans opened inside it, in the order they were opened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Raw {
    pub name: String,
    pub peer: String,
    pub children: Vec<Raw>,
}

impl Raw {
    pub fn new(name: &str, peer: &str, children: Vec<Raw>) -> Raw {
        Raw { name: name.into(), peer: peer.into(), children }
    }
}

/// One step of a canonical tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    pub name: String,
    pub peer: String,
    /// More than one step of this name happened under one instance of the parent.
    pub looped: bool,
    /// Some instances of the parent have no such step.
    pub opt: bool,
    pub children: Vec<Node>,
}

#[derive(Default)]
struct Arena {
    /// (name, peer, children) by index
    nodes: Vec<(String, String, Vec<usize>)>,
    roots: Vec<usize>,
}

impl Arena {
    fn raw(&self, i: usize) -> Raw {
        let (name, peer, children) = &self.nodes[i];
        Raw { name: name.clone(), peer: peer.clone(), children: children.iter().map(|&c| self.raw(c)).collect() }
    }
}

/// The index of a span's node in the arena, kept in the span's extensions.
struct Index(usize);

/// Reads the `peer` field and ignores every other one.
#[derive(Default)]
struct PeerVisitor(String);

impl Visit for PeerVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "peer" {
            self.0 = value.into();
        }
    }
    fn record_debug(&mut self, _: &Field, _: &dyn std::fmt::Debug) {}
}

/// The layer: every new span becomes a node under its real parent (explicit or contextual).
struct RecorderLayer(Arc<Mutex<Arena>>);

impl<S: Subscriber + for<'a> LookupSpan<'a>> Layer<S> for RecorderLayer {
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else { return };
        let mut peer = PeerVisitor::default();
        attrs.record(&mut peer);
        let parent = span.parent().and_then(|p| p.extensions().get::<Index>().map(|i| i.0));
        let mut arena = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let i = arena.nodes.len();
        arena.nodes.push((attrs.metadata().name().into(), peer.0, Vec::new()));
        match parent {
            Some(p) => arena.nodes[p].2.push(i),
            None => arena.roots.push(i),
        }
        span.extensions_mut().insert(Index(i));
    }
}

/// Run `f` under a fresh recorder and return what it recorded, as a canonical tree.
pub fn record(f: impl FnOnce()) -> Vec<Node> {
    // With a single live dispatcher, tracing-core decides a span site's interest from the dispatcher
    // of whichever thread reaches it first (`Dispatchers::rebuilder`, the "just one" case), so a
    // parallel thread without a recorder could switch the site off for the recording thread. A
    // second dispatcher, alive for the whole process, makes it rebuild over all of them.
    static HOLD: std::sync::OnceLock<tracing::Dispatch> = std::sync::OnceLock::new();
    HOLD.get_or_init(|| tracing::Dispatch::new(tracing::subscriber::NoSubscriber::default()));
    let arena = Arc::new(Mutex::new(Arena::default()));
    tracing::subscriber::with_default(Registry::default().with(RecorderLayer(arena.clone())), f);
    let arena = arena.lock().unwrap_or_else(|e| e.into_inner());
    canonical(&arena.roots.iter().map(|&r| arena.raw(r)).collect::<Vec<_>>())
}

/// The canonical form of a recorded forest: siblings of one name merge into one step (`looped`
/// when one parent instance had several), whose children are the merge of the children of all of
/// them; child order is the merge of the orders seen (ties by name); a step that some instances
/// of the parent lack is `opt`. The same run gives the same tree whatever the thread timing.
pub fn canonical(roots: &[Raw]) -> Vec<Node> {
    merge_children(&[roots])
}

/// The merged children of several instances of one parent.
fn merge_children(instances: &[&[Raw]]) -> Vec<Node> {
    struct Group<'a> {
        looped: bool,
        present: usize,
        members: Vec<&'a Raw>,
    }
    let mut groups: BTreeMap<&str, Group> = BTreeMap::new();
    let mut sequences: Vec<Vec<&str>> = Vec::new();
    for children in instances {
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        let mut sequence = Vec::new();
        for c in *children {
            let n = counts.entry(&c.name).or_insert(0);
            if *n == 0 {
                sequence.push(c.name.as_str());
            }
            *n += 1;
            groups.entry(&c.name).or_insert(Group { looped: false, present: 0, members: Vec::new() }).members.push(c);
        }
        for (name, n) in counts {
            let g = groups.get_mut(name).expect("counted above");
            g.present += 1;
            g.looped |= n > 1;
        }
        sequences.push(sequence);
    }
    merge_order(&sequences)
        .into_iter()
        .map(|name| {
            let g = &groups[name];
            let peer = g.members.iter().map(|m| m.peer.as_str()).min().unwrap_or("").to_string();
            let inner: Vec<&[Raw]> = g.members.iter().map(|m| m.children.as_slice()).collect();
            Node { name: name.into(), peer, looped: g.looped, opt: g.present < instances.len(), children: merge_children(&inner) }
        })
        .collect()
}

/// A topological merge of several sequences: a name goes after every name that precedes it in
/// some sequence; among the names that may go next, the one with the fewest unmet predecessors
/// (none, unless the sequences contradict each other), then the smallest name.
fn merge_order<'a>(sequences: &[Vec<&'a str>]) -> Vec<&'a str> {
    let mut before: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for seq in sequences {
        for (i, &name) in seq.iter().enumerate() {
            before.entry(name).or_default().extend(seq[..i].iter().copied());
        }
    }
    let mut remaining: BTreeSet<&str> = before.keys().copied().collect();
    let mut out = Vec::new();
    while let Some(&next) = remaining.iter().min_by_key(|n| (before[*n].iter().filter(|b| remaining.contains(*b)).count(), **n)) {
        remaining.remove(next);
        out.push(next);
    }
    out
}

/// The step label of a span name: `flow.mail_sync.fetch` shows as `fetch`.
fn label(name: &str) -> &str {
    name.strip_prefix("flow.").and_then(|rest| rest.split_once('.')).map_or(name, |(_, step)| step)
}

/// A Mermaid sequence diagram of a canonical tree: one participant per peer (in order of first
/// use). A root is the flow itself, and its peer is the caller: every arrow goes from there to the
/// step's peer, however deep the step is nested. A step whose peer is the caller's own is no
/// arrow but a block named after the step around its children: `loop <step>`, `opt <step>` (with
/// the loop inside when both), or `Note over <caller>: <step>` when it is neither. Other
/// repeated and optional steps are arrows inside `loop repeated` and `opt sometimes` blocks
/// (`opt` outside, `loop` inside).
pub fn render_mermaid(forest: &[Node]) -> String {
    fn peers<'a>(nodes: &'a [Node], out: &mut Vec<&'a str>) {
        for n in nodes {
            if !out.contains(&n.peer.as_str()) {
                out.push(&n.peer);
            }
            peers(&n.children, out);
        }
    }
    fn steps(out: &mut String, nodes: &[Node], caller: &str, depth: usize) {
        for n in nodes {
            let (mut d, pad) = (depth, |d: usize| "    ".repeat(d));
            let block = n.peer == caller;
            let (opt, looped) = if block { (format!("opt {}", label(&n.name)), format!("loop {}", label(&n.name))) } else { ("opt sometimes".into(), "loop repeated".into()) };
            if n.opt {
                out.push_str(&format!("{}{opt}\n", pad(d)));
                d += 1;
            }
            if n.looped {
                out.push_str(&format!("{}{looped}\n", pad(d)));
                d += 1;
            }
            if !block {
                out.push_str(&format!("{}{caller}->>{}: {}\n", pad(d), n.peer, label(&n.name)));
            } else if !n.opt && !n.looped {
                out.push_str(&format!("{}Note over {caller}: {}\n", pad(d), label(&n.name)));
            }
            steps(out, &n.children, caller, d);
            for closing in (depth..d).rev() {
                out.push_str(&format!("{}end\n", pad(closing)));
            }
        }
    }
    let mut participants = Vec::new();
    peers(forest, &mut participants);
    let mut out = String::from("sequenceDiagram\n");
    for p in participants {
        out.push_str(&format!("    participant {p}\n"));
    }
    for root in forest {
        steps(&mut out, &root.children, &root.peer, 1);
    }
    out
}

fn begin_marker(flow: &str) -> String {
    format!("<!-- dataflow:begin {flow} -->\n")
}

fn end_marker(flow: &str) -> String {
    format!("<!-- dataflow:end {flow} -->")
}

/// The generated block for a rendered diagram: a Mermaid fence.
fn fenced(rendered: &str) -> String {
    format!("```mermaid\n{rendered}```\n")
}

/// Where the block between the markers of a flow lies in the document.
fn block_range(doc: &str, flow: &str) -> Option<std::ops::Range<usize>> {
    let start = doc.find(&begin_marker(flow))? + begin_marker(flow).len();
    let end = start + doc[start..].find(&end_marker(flow))?;
    Some(start..end)
}

/// The text between the markers of a flow (a fenced block), `None` when a marker is missing.
pub fn current_block<'a>(doc: &'a str, flow: &str) -> Option<&'a str> {
    block_range(doc, flow).map(|r| &doc[r])
}

/// The document with the block between the markers of `flow` replaced by the rendered diagram.
pub fn update_block(doc: &str, flow: &str, rendered: &str) -> String {
    let range = block_range(doc, flow).unwrap_or_else(|| panic!("the document needs the markers {} and {}", begin_marker(flow).trim_end(), end_marker(flow)));
    format!("{}{}{}", &doc[..range.start], fenced(rendered), &doc[range.end..])
}

/// Compare the rendered diagram with the committed block of `path`; with `SHALE_UPDATE_DATAFLOW=1`
/// write the block instead of failing.
pub fn verify(path: &Path, flow: &str, rendered: &str) {
    verify_with(path, flow, rendered, std::env::var("SHALE_UPDATE_DATAFLOW").is_ok_and(|v| v == "1"));
}

/// `verify` with the decision to update as a parameter. Updating reads and rewrites the whole
/// document, so two flows of one doc updating in parallel would lose a block: one lock serialises them.
pub(crate) fn verify_with(path: &Path, flow: &str, rendered: &str, update: bool) {
    static UPDATE: Mutex<()> = Mutex::new(());
    let _guard = update.then(|| UPDATE.lock().unwrap_or_else(|e| e.into_inner()));
    let doc = crate::fsutil::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    if update {
        crate::fsutil::write_atomic(path, update_block(&doc, flow, rendered).as_bytes()).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        return;
    }
    assert_eq!(current_block(&doc, flow), Some(fenced(rendered).as_str()), "the dataflow diagram of {flow} in {} is out of date: run this test again with SHALE_UPDATE_DATAFLOW=1 to rewrite it, and commit the result", path.display());
}
