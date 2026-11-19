//! Tests of the dataflow recorder, canonical form and renderer (DEV-026). No containers needed.

use crate::dataflow::{canonical, current_block, record, render_mermaid, update_block, Node, Raw};

fn leaf(name: &str, peer: &str) -> Raw {
    Raw::new(name, peer, vec![])
}

fn names(nodes: &[Node]) -> Vec<&str> {
    nodes.iter().map(|n| n.name.as_str()).collect()
}

fn flow(children: Vec<Raw>) -> Vec<Raw> {
    vec![Raw::new("flow.t", "sync", children)]
}

// spec: DEV-026
#[test]
fn dev_026_children_nest_under_their_parent() {
    let tree = canonical(&flow(vec![Raw::new("flow.t.a", "imap", vec![leaf("flow.t.b", "store")])]));
    assert_eq!(tree.len(), 1);
    let root = &tree[0];
    assert_eq!((root.name.as_str(), root.peer.as_str()), ("flow.t", "sync"));
    assert_eq!(names(&root.children), ["flow.t.a"]);
    assert_eq!(names(&root.children[0].children), ["flow.t.b"]);
    assert!(!root.children[0].looped && !root.children[0].opt);
}

// spec: DEV-026
#[test]
fn dev_026_siblings_with_one_name_merge_into_a_loop() {
    let tree = canonical(&flow(vec![leaf("flow.t.a", "imap"), leaf("flow.t.a", "imap"), leaf("flow.t.b", "store")]));
    let kids = &tree[0].children;
    assert_eq!(names(kids), ["flow.t.a", "flow.t.b"]);
    assert!(kids[0].looped && !kids[1].looped);
}

// spec: DEV-026
#[test]
fn dev_026_a_merged_step_has_the_merged_children_of_all_instances() {
    let tree = canonical(&flow(vec![Raw::new("flow.t.a", "imap", vec![leaf("flow.t.x", "store")]), Raw::new("flow.t.a", "imap", vec![leaf("flow.t.y", "store"), leaf("flow.t.y", "store")])]));
    let a = &tree[0].children[0];
    assert_eq!(names(&a.children), ["flow.t.x", "flow.t.y"]);
    assert!(a.looped, "two instances of a under one parent: a loop");
    // each child is under one instance only, so both are optional; y happened twice in that one
    assert!(a.children[0].opt && a.children[1].opt);
    assert!(!a.children[0].looped && a.children[1].looped);
}

// spec: DEV-026
#[test]
fn dev_026_a_step_missing_under_some_instances_is_optional() {
    let tree = canonical(&flow(vec![Raw::new("flow.t.a", "imap", vec![leaf("flow.t.x", "store")]), Raw::new("flow.t.a", "imap", vec![])]));
    let x = &tree[0].children[0].children[0];
    assert!(x.opt && !x.looped);
    // present under every instance: not optional
    let tree = canonical(&flow(vec![Raw::new("flow.t.a", "imap", vec![leaf("flow.t.x", "store")]), Raw::new("flow.t.a", "imap", vec![leaf("flow.t.x", "store")])]));
    assert!(!tree[0].children[0].children[0].opt);
}

// spec: DEV-026
#[test]
fn dev_026_child_order_is_a_merge_of_the_orders_seen_with_ties_by_name() {
    let inst = |kids: &[&str]| Raw::new("flow.t.a", "imap", kids.iter().map(|k| leaf(k, "store")).collect());
    // [c, a] and [c, b]: c first; a and b are not ordered against each other: by name
    let tree = canonical(&flow(vec![inst(&["c", "a"]), inst(&["c", "b"])]));
    assert_eq!(names(&tree[0].children[0].children), ["c", "a", "b"]);
    // [b, d] and [a, b, c]: the orders agree on b before d and a before b before c; ties by name
    let tree = canonical(&flow(vec![inst(&["b", "d"]), inst(&["a", "b", "c"])]));
    assert_eq!(names(&tree[0].children[0].children), ["a", "b", "c", "d"]);
    // contradicting orders still give one deterministic answer, whichever instance came first
    let one = canonical(&flow(vec![inst(&["x", "y"]), inst(&["y", "x"])]));
    let two = canonical(&flow(vec![inst(&["y", "x"]), inst(&["x", "y"])]));
    assert_eq!(one, two);
}

fn sample() -> Vec<Node> {
    canonical(&flow(vec![
        leaf("flow.t.connect", "imap"),
        Raw::new("flow.t.fetch", "imap", vec![leaf("flow.t.write", "store")]),
        Raw::new("flow.t.fetch", "imap", vec![leaf("flow.t.write", "store"), leaf("flow.t.write", "store")]),
        Raw::new("flow.t.fetch", "imap", vec![]),
        leaf("flow.t.done", "sync"),
    ]))
}

// spec: DEV-026
#[test]
fn dev_026_the_diagram_text_of_a_small_tree() {
    let want = "sequenceDiagram
    participant sync
    participant imap
    participant store
    sync->>imap: connect
    loop repeated
        sync->>imap: fetch
        opt sometimes
            loop repeated
                sync->>store: write
            end
        end
    end
    Note over sync: done
";
    assert_eq!(render_mermaid(&sample()), want);
}

// spec: DEV-026
#[test]
fn dev_026_a_step_both_optional_and_repeated_has_the_opt_outside() {
    let tree = canonical(&flow(vec![Raw::new("flow.t.a", "imap", vec![leaf("flow.t.b", "store"), leaf("flow.t.b", "store")]), Raw::new("flow.t.a", "imap", vec![])]));
    let want = "sequenceDiagram
    participant sync
    participant imap
    participant store
    loop repeated
        sync->>imap: a
        opt sometimes
            loop repeated
                sync->>store: b
            end
        end
    end
";
    assert_eq!(render_mermaid(&tree), want);
}

// spec: DEV-026
#[test]
fn dev_026_output_is_the_same_whichever_order_the_siblings_arrive_in() {
    let fetch = |kids: Vec<Raw>| Raw::new("flow.t.fetch", "imap", kids);
    let one = canonical(&flow(vec![leaf("flow.t.connect", "imap"), fetch(vec![leaf("flow.t.write", "store")]), fetch(vec![]), leaf("flow.t.done", "sync")]));
    let two = canonical(&flow(vec![leaf("flow.t.connect", "imap"), fetch(vec![]), fetch(vec![leaf("flow.t.write", "store")]), leaf("flow.t.done", "sync")]));
    assert_eq!(one, two);
    assert_eq!(render_mermaid(&one), render_mermaid(&two));
}

const DOC: &str = "# Flow\n\nProse.\n\n<!-- dataflow:begin t -->\n```mermaid\nold\n```\n<!-- dataflow:end t -->\n\nMore prose.\n";

// spec: DEV-026
#[test]
fn dev_026_the_block_between_the_markers_is_replaced_and_the_prose_stays() {
    let new = update_block(DOC, "t", "sequenceDiagram\n    participant sync\n");
    assert_eq!(new, "# Flow\n\nProse.\n\n<!-- dataflow:begin t -->\n```mermaid\nsequenceDiagram\n    participant sync\n```\n<!-- dataflow:end t -->\n\nMore prose.\n");
    assert_eq!(current_block(&new, "t"), Some("```mermaid\nsequenceDiagram\n    participant sync\n```\n"));
    assert_eq!(update_block(&new, "t", "sequenceDiagram\n    participant sync\n"), new, "updating twice changes nothing");
    assert_eq!(current_block(DOC, "other"), None);
}

/// The steps of a small flow, run by two threads that take over the caller's dispatcher and span;
/// `swap` makes the second body run first.
fn run_two_threads(swap: bool) -> Vec<Node> {
    record(|| {
        let top = tracing::info_span!("flow.t", peer = "sync");
        let _in = top.enter();
        tracing::info_span!("flow.t.connect", peer = "imap").in_scope(|| {});
        let (dispatch, span) = (tracing::dispatcher::get_default(|d| d.clone()), tracing::Span::current());
        let bodies: [fn(); 2] = [
            || {
                tracing::info_span!("flow.t.select", peer = "imap").in_scope(|| {});
                tracing::info_span!("flow.t.write", peer = "store").in_scope(|| {});
            },
            || {
                tracing::info_span!("flow.t.select", peer = "imap").in_scope(|| {});
                tracing::info_span!("flow.t.write", peer = "store").in_scope(|| {});
            },
        ];
        std::thread::scope(|s| {
            let order = if swap { [1, 0] } else { [0, 1] };
            for i in order {
                let (dispatch, span, body) = (&dispatch, &span, bodies[i]);
                s.spawn(move || tracing::dispatcher::with_default(dispatch, || span.in_scope(body))).join().unwrap();
            }
        });
    })
}

// spec: DEV-026
#[test]
fn dev_026_a_recorded_run_is_the_same_whichever_thread_goes_first() {
    let one = run_two_threads(false);
    let two = run_two_threads(true);
    assert_eq!(one, two);
    assert_eq!(names(&one[0].children), ["flow.t.connect", "flow.t.select", "flow.t.write"], "the steps of the threads hang under the flow's span");
    assert!(one[0].children[1].looped && one[0].children[2].looped && !one[0].children[0].looped);
    assert_eq!(one[0].children[1].peer, "imap");
}

// spec: DEV-026
#[test]
fn dev_026_a_recorded_span_keeps_its_name_and_peer_and_no_other_field() {
    let tree = record(|| {
        tracing::info_span!("flow.t", peer = "sync", secret = "hunter2", count = 3).in_scope(|| {
            tracing::info_span!("flow.t.a", peer = "imap", folder = "INBOX").in_scope(|| {});
            tracing::info_span!("flow.t.b").in_scope(|| {});
        });
    });
    let want = vec![Node {
        name: "flow.t".into(),
        peer: "sync".into(),
        looped: false,
        opt: false,
        children: vec![Node { name: "flow.t.a".into(), peer: "imap".into(), looped: false, opt: false, children: vec![] }, Node { name: "flow.t.b".into(), peer: String::new(), looped: false, opt: false, children: vec![] }],
    }];
    assert_eq!(tree, want);
}

// spec: DEV-026
#[test]
fn dev_026_a_step_of_the_callers_own_peer_is_a_block_and_arrows_start_at_the_caller() {
    let folder = |kids: Vec<Raw>| Raw::new("flow.t.folder", "sync", kids);
    let select = || leaf("flow.t.select", "imap");
    let write = || Raw::new("flow.t.fetch", "imap", vec![leaf("flow.t.write", "store")]);
    // two folders: a loop block, and a step only one of them has is optional
    let tree = canonical(&flow(vec![folder(vec![select(), write()]), folder(vec![select()])]));
    let want = "sequenceDiagram
    participant sync
    participant imap
    participant store
    loop folder
        sync->>imap: select
        opt sometimes
            sync->>imap: fetch
            sync->>store: write
        end
    end
";
    assert_eq!(render_mermaid(&tree), want);
    // one folder: a note, the children at the same depth
    let tree = canonical(&flow(vec![folder(vec![select()])]));
    assert_eq!(render_mermaid(&tree), "sequenceDiagram\n    participant sync\n    participant imap\n    Note over sync: folder\n    sync->>imap: select\n");
    // both optional and repeated: opt outside, loop inside, both named after the step
    let tree = canonical(&flow(vec![Raw::new("flow.t.a", "imap", vec![folder(vec![]), folder(vec![])]), Raw::new("flow.t.a", "imap", vec![])]));
    let text = render_mermaid(&tree);
    assert!(text.contains("        opt folder\n            loop folder\n            end\n        end\n"), "{text}");
}
