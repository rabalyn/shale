//! Connection anchors and automatic arrangement for drawings.
//!
//! Pure geometry, no UI: shapes are (kind, world rect), connections are (from, to) indices.
//! - `anchor_ratios`: the 8 points a connection can attach to (corners / edge middles, or the
//!   matching points on the outline of diamonds and ellipses), as fractions of the bounding box.
//! - `count_crossings`: how many connections cross each other or run through another shape.
//! - `auto_layout`: layered arrangement (Sugiyama style) that searches for the order with the
//!   fewest crossings. The search stops when it reaches 0 crossings, stops improving, repeats an
//!   order it has already seen, or hits a fixed step limit — so it can never loop forever.
//! - `spread`: push shapes apart / pull them together without overlapping shapes and without
//!   adding crossings (falls back to a smaller step, or refuses).

use eframe::egui::{Pos2, Rect, Vec2};
use std::collections::{HashMap, HashSet};

/// The 8 attachment points of a shape, as fractions (0..1) of its bounding box.
pub fn anchor_ratios(kind: &str) -> [[f32; 2]; 8] {
    match kind {
        "diamond" => [[0.5, 0.0], [0.75, 0.25], [1.0, 0.5], [0.75, 0.75], [0.5, 1.0], [0.25, 0.75], [0.0, 0.5], [0.25, 0.25]],
        "ellipse" => {
            let mut out = [[0.0; 2]; 8];
            for (i, o) in out.iter_mut().enumerate() {
                let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::FRAC_PI_4;
                *o = [0.5 + 0.5 * a.cos(), 0.5 + 0.5 * a.sin()];
            }
            out
        }
        // rectangles, text and everything else: corners and edge middles (clockwise from top)
        _ => [[0.5, 0.0], [1.0, 0.0], [1.0, 0.5], [1.0, 1.0], [0.5, 1.0], [0.0, 1.0], [0.0, 0.5], [0.0, 0.0]],
    }
}

pub fn anchor_point(r: Rect, ratio: [f32; 2]) -> Pos2 {
    Pos2::new(r.min.x + r.width() * ratio[0], r.min.y + r.height() * ratio[1])
}

/// Nearest anchor of a shape to `p`: (index, point, distance).
pub fn nearest_anchor(kind: &str, r: Rect, p: Pos2) -> (usize, Pos2, f32) {
    let mut best = (0, Pos2::ZERO, f32::MAX);
    for (i, ratio) in anchor_ratios(kind).iter().enumerate() {
        let q = anchor_point(r, *ratio);
        let d = (q - p).length();
        if d < best.2 {
            best = (i, q, d);
        }
    }
    best
}

/// The pair of anchors that gives the shortest connection between two shapes.
/// Corners are slightly penalised so connections prefer edge middles.
pub fn best_anchor_pair(ka: &str, ra: Rect, kb: &str, rb: Rect) -> (usize, usize) {
    let aa = anchor_ratios(ka);
    let ab = anchor_ratios(kb);
    let mut best = (0, 0, f32::MAX);
    for (i, x) in aa.iter().enumerate() {
        for (j, y) in ab.iter().enumerate() {
            let mut d = (anchor_point(ra, *x) - anchor_point(rb, *y)).length();
            if i % 2 == 1 {
                d += 6.0;
            }
            if j % 2 == 1 {
                d += 6.0;
            }
            if d < best.2 {
                best = (i, j, d);
            }
        }
    }
    (best.0, best.1)
}

fn segments_cross(a: Pos2, b: Pos2, c: Pos2, d: Pos2) -> bool {
    let cross = |o: Pos2, p: Pos2, q: Pos2| (p.x - o.x) * (q.y - o.y) - (p.y - o.y) * (q.x - o.x);
    let d1 = cross(c, d, a);
    let d2 = cross(c, d, b);
    let d3 = cross(a, b, c);
    let d4 = cross(a, b, d);
    ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0)) && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
}

fn segment_hits_rect(a: Pos2, b: Pos2, r: Rect) -> bool {
    let r = r.shrink(2.0);
    if r.width() <= 0.0 || r.height() <= 0.0 {
        return false;
    }
    if r.contains(a) || r.contains(b) {
        return true;
    }
    let c = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()];
    (0..4).any(|i| segments_cross(a, b, c[i], c[(i + 1) % 4]))
}

/// The straight connection segments for a layout (using the best anchor pair for each).
pub fn connection_segments(nodes: &[(String, Rect)], edges: &[(usize, usize)]) -> Vec<(Pos2, Pos2)> {
    edges
        .iter()
        .map(|&(a, b)| {
            let (i, j) = best_anchor_pair(&nodes[a].0, nodes[a].1, &nodes[b].0, nodes[b].1);
            (anchor_point(nodes[a].1, anchor_ratios(&nodes[a].0)[i]), anchor_point(nodes[b].1, anchor_ratios(&nodes[b].0)[j]))
        })
        .collect()
}

/// Number of "overlapping connections": pairs of connections that cross, plus connections
/// that run through a shape they don't belong to.
pub fn count_crossings(nodes: &[(String, Rect)], edges: &[(usize, usize)]) -> usize {
    let segs = connection_segments(nodes, edges);
    let mut n = 0;
    for i in 0..edges.len() {
        for j in (i + 1)..edges.len() {
            let (a, b) = edges[i];
            let (c, d) = edges[j];
            if a == c || a == d || b == c || b == d {
                continue; // connections sharing a shape meet there; that's fine
            }
            if segments_cross(segs[i].0, segs[i].1, segs[j].0, segs[j].1) {
                n += 1;
            }
        }
        for (k, node) in nodes.iter().enumerate() {
            if k != edges[i].0 && k != edges[i].1 && segment_hits_rect(segs[i].0, segs[i].1, node.1) {
                n += 1;
            }
        }
    }
    n
}

fn overlaps(rects: &[Rect], margin: f32) -> bool {
    for i in 0..rects.len() {
        for j in (i + 1)..rects.len() {
            if rects[i].expand(margin / 2.0).intersects(rects[j].expand(margin / 2.0)) {
                return true;
            }
        }
    }
    false
}

pub struct LayoutResult {
    pub rects: Vec<Rect>,
    pub crossings_before: usize,
    pub crossings_after: usize,
    /// How many ordering sweeps the search ran before it stopped.
    pub sweeps: usize,
}

/// Arrange shapes in layers along their connections with as few crossings as possible.
/// Returns None when there is nothing to arrange.
pub fn auto_layout(nodes: &[(String, Rect)], edges: &[(usize, usize)]) -> Option<LayoutResult> {
    if nodes.len() < 2 {
        return None;
    }
    let before = count_crossings(nodes, edges);
    let origin = nodes.iter().map(|n| n.1).reduce(|a, b| a.union(b))?.min;
    let mut best: Option<(usize, f32, Vec<Rect>, usize)> = None;
    for vertical in [true, false] {
        let (rects, sweeps) = layered(nodes, edges, vertical, origin);
        let placed: Vec<(String, Rect)> = nodes.iter().zip(rects.iter()).map(|(n, r)| (n.0.clone(), *r)).collect();
        let c = count_crossings(&placed, edges);
        let length: f32 = connection_segments(&placed, edges).iter().map(|(a, b)| (*b - *a).length()).sum();
        let better = match &best {
            None => true,
            Some((bc, bl, _, _)) => c < *bc || (c == *bc && length < *bl * 0.97),
        };
        if better {
            best = Some((c, length, rects, sweeps));
        }
    }
    let (after, _, rects, sweeps) = best?;
    Some(LayoutResult { rects, crossings_before: before, crossings_after: after, sweeps })
}

/// One layered layout. `vertical`: layers go top to bottom, else left to right.
fn layered(nodes: &[(String, Rect)], edges: &[(usize, usize)], vertical: bool, origin: Pos2) -> (Vec<Rect>, usize) {
    let n = nodes.len();
    // --- 1. remove cycles (reverse back edges found by DFS, starting from the top-left shapes)
    let mut adj: Vec<Vec<usize>> = vec![vec![]; n];
    let mut seen_pairs = HashSet::new();
    for &(a, b) in edges {
        if a != b && seen_pairs.insert((a, b)) {
            adj[a].push(b);
        }
    }
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| {
        let (pa, pb) = (nodes[a].1.min, nodes[b].1.min);
        (pa.y + pa.x).partial_cmp(&(pb.y + pb.x)).unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut state = vec![0u8; n]; // 0 new, 1 on stack, 2 done
    let mut dag: Vec<(usize, usize)> = Vec::new();
    fn dfs(v: usize, adj: &[Vec<usize>], state: &mut [u8], dag: &mut Vec<(usize, usize)>) {
        state[v] = 1;
        for &w in &adj[v] {
            match state[w] {
                0 => {
                    dag.push((v, w));
                    dfs(w, adj, state, dag);
                }
                1 => dag.push((w, v)), // back edge: reverse it
                _ => dag.push((v, w)),
            }
        }
        state[v] = 2;
    }
    for &v in &order {
        if state[v] == 0 {
            dfs(v, &adj, &mut state, &mut dag);
        }
    }
    // --- 2. layers by longest path
    let mut layer = vec![0usize; n];
    for _ in 0..n {
        let mut changed = false;
        for &(a, b) in &dag {
            if layer[b] < layer[a] + 1 {
                layer[b] = layer[a] + 1;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let connected: Vec<bool> = (0..n).map(|v| dag.iter().any(|&(a, b)| a == v || b == v)).collect();
    // --- 3. dummy nodes for long connections, so the ordering can route around shapes
    let mut layer_of: Vec<usize> = layer.clone();
    let mut chains: Vec<(usize, usize)> = Vec::new(); // edges between consecutive layers
    for &(a, b) in &dag {
        let mut prev = a;
        for l in (layer[a] + 1)..layer[b] {
            layer_of.push(l);
            let d = layer_of.len() - 1;
            chains.push((prev, d));
            prev = d;
        }
        chains.push((prev, b));
    }
    let total = layer_of.len();
    let max_layer = layer_of.iter().copied().max().unwrap_or(0);
    let mut layers: Vec<Vec<usize>> = vec![vec![]; max_layer + 1];
    for v in 0..total {
        if v >= n || connected[v] {
            layers[layer_of[v]].push(v);
        }
    }
    // initial order: keep the current reading order (left to right / top to bottom)
    let pos_key = |v: usize| -> f32 {
        if v < n {
            let c = nodes[v].1.center();
            if vertical {
                c.x
            } else {
                c.y
            }
        } else {
            0.0
        }
    };
    for l in layers.iter_mut() {
        l.sort_by(|&a, &b| pos_key(a).partial_cmp(&pos_key(b)).unwrap_or(std::cmp::Ordering::Equal));
    }
    let up: Vec<Vec<usize>> = (0..total).map(|v| chains.iter().filter(|e| e.1 == v).map(|e| e.0).collect()).collect();
    let down: Vec<Vec<usize>> = (0..total).map(|v| chains.iter().filter(|e| e.0 == v).map(|e| e.1).collect()).collect();
    let count = |layers: &Vec<Vec<usize>>| -> usize {
        let mut idx = vec![0usize; total];
        for l in layers {
            for (i, v) in l.iter().enumerate() {
                idx[*v] = i;
            }
        }
        let mut c = 0;
        for l in 0..layers.len().saturating_sub(1) {
            let es: Vec<(usize, usize)> = chains.iter().filter(|e| layer_of[e.0] == l).map(|e| (idx[e.0], idx[e.1])).collect();
            for i in 0..es.len() {
                for j in (i + 1)..es.len() {
                    if (es[i].0 < es[j].0 && es[i].1 > es[j].1) || (es[i].0 > es[j].0 && es[i].1 < es[j].1) {
                        c += 1;
                    }
                }
            }
        }
        c
    };
    // --- 4. barycenter sweeps with an automatic stop
    let mut best_layers = layers.clone();
    let mut best_c = count(&layers);
    let mut seen: HashSet<Vec<Vec<usize>>> = HashSet::new();
    seen.insert(layers.clone());
    let mut no_gain = 0;
    let mut sweeps = 0;
    const MAX_SWEEPS: usize = 40;
    while best_c > 0 && no_gain < 4 && sweeps < MAX_SWEEPS {
        sweeps += 1;
        let downward = sweeps % 2 == 1;
        let range: Vec<usize> = if downward { (1..layers.len()).collect() } else { (0..layers.len().saturating_sub(1)).rev().collect() };
        for l in range {
            let mut idx = vec![0f32; total];
            let refl = if downward { l - 1 } else { l + 1 };
            for (i, v) in layers[refl].iter().enumerate() {
                idx[*v] = i as f32;
            }
            let neighbours = if downward { &up } else { &down };
            let cur: HashMap<usize, f32> = layers[l].iter().enumerate().map(|(i, v)| (*v, i as f32)).collect();
            let mut keyed: Vec<(f32, usize)> = layers[l]
                .iter()
                .map(|&v| {
                    let ns = &neighbours[v];
                    let k = if ns.is_empty() { cur[&v] } else { ns.iter().map(|w| idx[*w]).sum::<f32>() / ns.len() as f32 };
                    (k, v)
                })
                .collect();
            keyed.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
            layers[l] = keyed.into_iter().map(|(_, v)| v).collect();
        }
        if !seen.insert(layers.clone()) {
            break; // this order was already tried: we're going round in circles
        }
        let c = count(&layers);
        if c < best_c {
            best_c = c;
            best_layers = layers.clone();
            no_gain = 0;
        } else {
            no_gain += 1;
        }
    }
    let layers = best_layers;
    // --- 5. coordinates
    let gap_layer = 90.0;
    let gap_item = 60.0;
    let size = |v: usize| -> Vec2 {
        if v < n {
            nodes[v].1.size()
        } else {
            Vec2::splat(10.0)
        }
    };
    let along = |s: Vec2| if vertical { s.y } else { s.x };
    let across = |s: Vec2| if vertical { s.x } else { s.y };
    let mut rects = nodes.iter().map(|n| n.1).collect::<Vec<_>>();
    let mut cursor_layer = 0.0;
    let widths: Vec<f32> = layers.iter().map(|l| l.iter().map(|v| across(size(*v))).sum::<f32>() + gap_item * (l.len().max(1) - 1) as f32).collect();
    let widest = widths.iter().copied().fold(0.0, f32::max);
    for (li, l) in layers.iter().enumerate() {
        let depth = l.iter().map(|v| along(size(*v))).fold(0.0, f32::max);
        let mut cursor = (widest - widths[li]) / 2.0;
        for &v in l {
            let s = size(v);
            if v < n {
                let a = cursor_layer + (depth - along(s)) / 2.0;
                let min = if vertical { Pos2::new(cursor, a) } else { Pos2::new(a, cursor) };
                rects[v] = Rect::from_min_size(origin + min.to_vec2(), s);
            }
            cursor += across(s) + gap_item;
        }
        cursor_layer += depth + gap_layer;
    }
    // shapes without connections: a tidy row after the layers
    let mut cursor = 0.0;
    for v in 0..n {
        if !connected[v] {
            let s = size(v);
            let min = if vertical { Pos2::new(cursor, cursor_layer) } else { Pos2::new(cursor_layer, cursor) };
            rects[v] = Rect::from_min_size(origin + min.to_vec2(), s);
            cursor += across(s) + gap_item;
        }
    }
    (rects, sweeps)
}

pub enum SpreadOutcome {
    /// New rects and the factor that was actually used.
    Moved(Vec<Rect>, f32),
    /// Couldn't move without overlapping shapes or adding crossings.
    Blocked(&'static str),
}

/// Scale the distances between shape centres by `factor` (>1 spreads out, <1 pulls together).
/// Shapes keep their size. If the full step would make shapes overlap or connections cross
/// more, smaller steps are tried (binary search, at most 12 tries); if none works, nothing moves.
pub fn spread(nodes: &[(String, Rect)], edges: &[(usize, usize)], factor: f32) -> SpreadOutcome {
    if nodes.len() < 2 {
        return SpreadOutcome::Blocked("Select at least two shapes");
    }
    let centre = nodes.iter().fold(Vec2::ZERO, |acc, n| acc + n.1.center().to_vec2()) / nodes.len() as f32;
    let before = count_crossings(nodes, edges);
    let apply = |f: f32| -> Vec<Rect> { nodes.iter().map(|n| Rect::from_center_size((centre + (n.1.center().to_vec2() - centre) * f).to_pos2(), n.1.size())).collect() };
    let ok = |rects: &Vec<Rect>| -> bool {
        if overlaps(rects, 16.0) && !overlaps(&nodes.iter().map(|n| n.1).collect::<Vec<_>>(), 16.0) {
            return false;
        }
        let placed: Vec<(String, Rect)> = nodes.iter().zip(rects.iter()).map(|(n, r)| (n.0.clone(), *r)).collect();
        count_crossings(&placed, edges) <= before
    };
    let full = apply(factor);
    if ok(&full) {
        return SpreadOutcome::Moved(full, factor);
    }
    // find the biggest part of the step that still works
    let (mut lo, mut hi) = (1.0f32, factor);
    let mut found: Option<(Vec<Rect>, f32)> = None;
    for _ in 0..12 {
        let mid = (lo + hi) / 2.0;
        let r = apply(mid);
        if ok(&r) {
            found = Some((r, mid));
            lo = mid;
        } else {
            hi = mid;
        }
    }
    match found {
        Some((r, f)) if (f - 1.0).abs() > 0.01 => SpreadOutcome::Moved(r, f),
        _ => SpreadOutcome::Blocked(if factor < 1.0 { "Shapes are as close as they can be without overlapping" } else { "Spreading further would make connections cross" }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: f32, y: f32) -> Rect {
        Rect::from_min_size(Pos2::new(x, y), Vec2::new(100.0, 60.0))
    }
    fn node(x: f32, y: f32) -> (String, Rect) {
        ("rectangle".to_string(), r(x, y))
    }

    // spec: DRAW-004
    #[test]
    fn anchors_are_on_the_shape() {
        let rect = r(0.0, 0.0);
        let a = anchor_ratios("rectangle");
        assert_eq!(anchor_point(rect, a[0]), Pos2::new(50.0, 0.0));
        assert_eq!(anchor_point(rect, a[3]), Pos2::new(100.0, 60.0));
        let (i, p, _) = nearest_anchor("rectangle", rect, Pos2::new(103.0, 31.0));
        assert_eq!(i, 2);
        assert_eq!(p, Pos2::new(100.0, 30.0));
        let e = anchor_ratios("ellipse");
        assert!((e[0][0] - 0.5).abs() < 1e-4 && e[0][1].abs() < 1e-4);
        let (i, j) = best_anchor_pair("rectangle", r(0.0, 0.0), "rectangle", r(300.0, 0.0));
        assert_eq!((i, j), (2, 6), "facing edge middles");
    }

    // spec: DRAW-004
    #[test]
    fn crossings_are_counted() {
        // an X: 0-3 and 1-2 cross
        let nodes = vec![node(0.0, 0.0), node(400.0, 0.0), node(0.0, 300.0), node(400.0, 300.0)];
        assert_eq!(count_crossings(&nodes, &[(0, 3), (1, 2)]), 1);
        assert_eq!(count_crossings(&nodes, &[(0, 1), (2, 3)]), 0);
        // connection through another shape
        let nodes2 = vec![node(0.0, 0.0), node(200.0, 0.0), node(400.0, 0.0)];
        assert_eq!(count_crossings(&nodes2, &[(0, 2)]), 1);
    }

    // spec: DRAW-004
    #[test]
    fn auto_layout_untangles_and_stops() {
        // a crossed layout: two rows with the connections in an X
        let nodes = vec![node(0.0, 0.0), node(400.0, 0.0), node(0.0, 300.0), node(400.0, 300.0)];
        let edges = vec![(0, 3), (1, 2)];
        let res = auto_layout(&nodes, &edges).unwrap();
        assert_eq!(res.crossings_before, 1);
        assert_eq!(res.crossings_after, 0);
        assert!(res.sweeps <= 40);
        assert!(!overlaps(&res.rects, 1.0), "arranged shapes must not overlap");
        // a cycle doesn't loop forever
        let ring = vec![node(0.0, 0.0), node(300.0, 0.0), node(150.0, 300.0)];
        let res = auto_layout(&ring, &[(0, 1), (1, 2), (2, 0)]).unwrap();
        assert!(res.sweeps <= 40);
        assert!(!overlaps(&res.rects, 1.0));
    }

    // spec: DRAW-004
    #[test]
    fn bigger_graph_gets_no_worse() {
        let mut nodes = vec![];
        for i in 0..8 {
            nodes.push(node((i % 4) as f32 * 220.0, (i / 4) as f32 * 260.0));
        }
        let edges = vec![(0, 5), (1, 4), (2, 7), (3, 6), (0, 6), (4, 2)];
        let res = auto_layout(&nodes, &edges).unwrap();
        assert!(res.crossings_after <= res.crossings_before);
        assert!(!overlaps(&res.rects, 1.0));
    }

    // spec: DRAW-004
    #[test]
    fn spreading_keeps_shapes_apart() {
        let nodes = vec![node(0.0, 0.0), node(200.0, 0.0), node(100.0, 200.0)];
        let edges = vec![(0, 1), (1, 2)];
        match spread(&nodes, &edges, 1.25) {
            SpreadOutcome::Moved(rs, f) => {
                assert!((f - 1.25).abs() < 1e-3);
                assert!(rs[1].center().x - rs[0].center().x > 240.0);
                assert_eq!(rs[0].size(), nodes[0].1.size());
            }
            SpreadOutcome::Blocked(_) => panic!("should spread"),
        }
        // pulling together stops before shapes overlap
        let mut cur = nodes.clone();
        for _ in 0..20 {
            match spread(&cur, &edges, 0.8) {
                SpreadOutcome::Moved(rs, _) => cur = cur.iter().zip(rs).map(|(n, r)| (n.0.clone(), r)).collect(),
                SpreadOutcome::Blocked(_) => break,
            }
        }
        let rects: Vec<Rect> = cur.iter().map(|n| n.1).collect();
        assert!(!overlaps(&rects, 15.0));
    }
}
