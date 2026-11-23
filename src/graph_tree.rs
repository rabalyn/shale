//! The repulsion of the graph view for many notes (PERF-003, PERF-007): every note pushes every
//! other away with a force of `2600 / distance` (the pair rule of `GraphView::step`). Worked out
//! pair by pair that is quadratic; with a quadtree (Barnes-Hut) a far group of notes pushes as one
//! note at its centre of mass, which gives nearly the same forces in `O(n log n)`.

use eframe::egui::Vec2;

/// Below this many notes the pairs are worked out one by one: the layout is the exact one.
pub const EXACT_BELOW: usize = 300;

/// A cell counts as one far note when its size over its distance is below this.
const THETA: f32 = 0.5;
/// Notes that sit on top of each other stop splitting a cell at this depth.
const MAX_DEPTH: u32 = 20;
/// The strength of the push and the shortest distance it is worked out for (as in the pair rule).
const PUSH: f32 = 2600.0;
const MIN_DIST2: f32 = 25.0;

/// The push of a note at `b` on a note at `a`.
fn push(a: Vec2, b: Vec2, mass: f32) -> Vec2 {
    let d = a - b;
    let dist2 = d.length_sq().max(MIN_DIST2);
    d / dist2.sqrt() * (PUSH * mass / dist2)
}

struct Cell {
    centre: Vec2,
    half: f32,
    mass: f32,
    /// the sum of the positions of the notes inside, then their centre of mass
    sum: Vec2,
    children: [u32; 4],
    /// the first note of a leaf, `NONE` for none; the others follow by `Tree::next` (several only at the deepest level)
    head: u32,
}

/// No note, no next note.
const NONE: u32 = u32::MAX;

impl Cell {
    fn new(centre: Vec2, half: f32) -> Cell {
        Cell { centre, half, mass: 0.0, sum: Vec2::ZERO, children: [0; 4], head: NONE }
    }

    fn is_leaf(&self) -> bool {
        self.children == [0; 4]
    }

    fn quadrant(&self, p: Vec2) -> usize {
        usize::from(p.x >= self.centre.x) + 2 * usize::from(p.y >= self.centre.y)
    }
}

/// The quadtree. It is built again into the same buffers every step, so a step allocates nothing
/// once they have grown (PERF-003).
#[derive(Default)]
pub struct Tree {
    cells: Vec<Cell>,
    /// the note after a note in the same leaf, `NONE` for the last
    next: Vec<u32>,
}

impl Tree {
    #[cfg(test)]
    pub fn build(pos: &[Vec2]) -> Tree {
        let mut tree = Tree::default();
        tree.build_into(pos);
        tree
    }

    /// Builds the tree of `pos` into this tree's buffers.
    pub fn build_into(&mut self, pos: &[Vec2]) {
        let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
        for p in pos {
            lo = lo.min(*p);
            hi = hi.max(*p);
        }
        let half = ((hi - lo).max_elem() / 2.0).max(1.0) + 1.0;
        self.cells.clear();
        self.cells.push(Cell::new((lo + hi) / 2.0, half));
        self.next.clear();
        self.next.resize(pos.len(), NONE);
        for i in 0..pos.len() {
            self.insert(pos, i as u32);
        }
        for c in &mut self.cells {
            if c.mass > 0.0 {
                c.sum /= c.mass;
            }
        }
    }

    /// Puts a note at the front of a leaf.
    fn add_body(&mut self, at: usize, body: u32) {
        self.next[body as usize] = self.cells[at].head;
        self.cells[at].head = body;
    }

    fn insert(&mut self, pos: &[Vec2], body: u32) {
        let p = pos[body as usize];
        let mut at = 0usize;
        let mut depth = 0;
        loop {
            self.cells[at].mass += 1.0;
            self.cells[at].sum += p;
            if self.cells[at].is_leaf() {
                if self.cells[at].head == NONE || depth >= MAX_DEPTH {
                    self.add_body(at, body);
                    return;
                }
                // a leaf that holds a note splits: that note goes one level down
                let mut held = std::mem::replace(&mut self.cells[at].head, NONE);
                while held != NONE {
                    let h = held;
                    held = self.next[h as usize];
                    let q = self.cells[at].quadrant(pos[h as usize]);
                    let child = self.child(at, q);
                    self.cells[child].mass += 1.0;
                    self.cells[child].sum += pos[h as usize];
                    self.add_body(child, h);
                }
            }
            let q = self.cells[at].quadrant(p);
            at = self.child(at, q);
            depth += 1;
        }
    }

    fn child(&mut self, at: usize, q: usize) -> usize {
        if self.cells[at].children[q] == 0 {
            let (c, h) = (self.cells[at].centre, self.cells[at].half / 2.0);
            let centre = c + Vec2::new(if q & 1 == 1 { h } else { -h }, if q & 2 == 2 { h } else { -h });
            self.cells.push(Cell::new(centre, h));
            self.cells[at].children[q] = (self.cells.len() - 1) as u32;
        }
        self.cells[at].children[q] as usize
    }

    /// The push of all the other notes on note `i` at `pos[i]`.
    /// (`todo` is the list of cells still to look at, kept by the caller from note to note.)
    pub fn force_on(&self, pos: &[Vec2], i: usize, todo: &mut Vec<usize>) -> Vec2 {
        let mut total = Vec2::ZERO;
        todo.clear();
        todo.push(0);
        while let Some(at) = todo.pop() {
            let cell = &self.cells[at];
            if cell.mass == 0.0 {
                continue;
            }
            if cell.is_leaf() {
                let mut b = cell.head;
                while b != NONE {
                    if b as usize != i {
                        total += push(pos[i], pos[b as usize], 1.0);
                    }
                    b = self.next[b as usize];
                }
                continue;
            }
            let d = pos[i] - cell.sum;
            // far enough (the note itself may be inside the cell: then it is never far)
            let inside = (pos[i].x - cell.centre.x).abs() <= cell.half && (pos[i].y - cell.centre.y).abs() <= cell.half;
            if !inside && cell.half * 2.0 * cell.half * 2.0 < THETA * THETA * d.length_sq() {
                total += push(pos[i], cell.sum, cell.mass);
            } else {
                todo.extend(cell.children.iter().filter(|c| **c != 0).map(|c| *c as usize));
            }
        }
        total
    }
}

/// The push of every note on every note, by the pair rule: `O(n²)`. It fills `force`, which keeps
/// its buffer from step to step, so a small graph allocates nothing per step (PERF-003).
fn exact_forces_into(pos: &[Vec2], force: &mut Vec<Vec2>) {
    force.clear();
    force.resize(pos.len(), Vec2::ZERO);
    for i in 0..pos.len() {
        for j in (i + 1)..pos.len() {
            let f = push(pos[i], pos[j], 1.0);
            force[i] += f;
            force[j] -= f;
        }
    }
}

/// The push of every note on every note, by the pair rule: `O(n²)`.
#[cfg(test)]
pub fn exact_forces(pos: &[Vec2]) -> Vec<Vec2> {
    let mut force = Vec::new();
    exact_forces_into(pos, &mut force);
    force
}

/// The push on every note, into `force` (whose buffer is reused): the exact one for a small
/// graph, the quadtree's for a big one.
pub fn forces_into(pos: &[Vec2], force: &mut Vec<Vec2>, scratch: &mut Scratch) {
    if pos.len() < EXACT_BELOW {
        return exact_forces_into(pos, force);
    }
    scratch.tree.build_into(pos);
    force.clear();
    force.extend((0..pos.len()).map(|i| scratch.tree.force_on(pos, i, &mut scratch.todo)));
}

/// The buffers `forces_into` works in, kept from step to step.
#[derive(Default)]
pub struct Scratch {
    tree: Tree,
    todo: Vec<usize>,
}

/// The push on every note, in a new vector.
#[cfg(test)]
pub fn forces(pos: &[Vec2]) -> Vec<Vec2> {
    let mut force = Vec::new();
    forces_into(pos, &mut force, &mut Scratch::default());
    force
}

#[cfg(test)]
mod tests {
    use super::*;

    /// n notes on a deterministic spiral with some clumps, as the graph starts.
    fn spiral(n: usize) -> Vec<Vec2> {
        (0..n)
            .map(|i| {
                let a = i as f32 * 2.399;
                let r = 30.0 * (i as f32 + 1.0).sqrt() * if i % 7 == 0 { 0.2 } else { 1.0 };
                Vec2::new(a.cos() * r, a.sin() * r)
            })
            .collect()
    }

    // spec: PERF-003
    #[test]
    fn perf_003_the_quadtree_pushes_nearly_as_the_pairs_do() {
        let pos = spiral(1500);
        let exact = exact_forces(&pos);
        let tree = Tree::build(&pos);
        let mut todo = Vec::new();
        let (mut err, mut size, mut worst) = (0.0f32, 0.0f32, 0.0f32);
        for (i, e) in exact.iter().enumerate() {
            let f = tree.force_on(&pos, i, &mut todo);
            let diff = (f - *e).length();
            err += diff;
            size += e.length();
            worst = worst.max(diff);
        }
        assert!(err / size < 0.05, "the forces are {:.1} % off in all", 100.0 * err / size);
        assert!(worst < 0.3 * size / pos.len() as f32, "no note is pushed far off the mean push: {worst} against {}", size / pos.len() as f32);
        // the pushes of all notes on all add up to about nothing, as they do for the pairs
        let sum = (0..pos.len()).map(|i| tree.force_on(&pos, i, &mut todo)).fold(Vec2::ZERO, |a, b| a + b);
        assert!(sum.length() < 0.05 * size, "{sum:?}");
    }

    // spec: PERF-003
    #[test]
    fn perf_003_a_small_graph_is_worked_out_pair_by_pair_and_notes_on_one_spot_do_not_hang() {
        let pos = spiral(EXACT_BELOW - 1);
        assert_eq!(forces(&pos), exact_forces(&pos), "the layout of a small graph is the exact one");
        // many notes on the same spot (the pair rule clamps the distance, so they push by the clamp)
        let same = vec![Vec2::new(5.0, 5.0); 400];
        let f = forces(&same);
        assert_eq!(f.len(), 400);
        assert!(f.iter().all(|v| v.x.is_finite() && v.y.is_finite()));
        assert!(forces(&[]).is_empty());
        assert_eq!(forces(&[Vec2::ZERO]), [Vec2::ZERO]);
    }
}
