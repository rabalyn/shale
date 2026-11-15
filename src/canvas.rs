//! Obsidian Canvas files (`Name.canvas`, the open JSON Canvas 1.0 format, <https://jsoncanvas.org>).
//!
//! A canvas is `{"nodes": [...], "edges": [...]}`: cards (`text`, `file`, `link`, `group`) with a
//! position and size, and edges from one card's side to another's. Shale shows a canvas in the
//! drawing view by turning it into an Excalidraw scene (read-only), and can convert it into an
//! editable `Name.excalidraw.md` drawing next to it. The canvas file itself is never written.

use crate::drawing::Scene;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Front matter key that marks a drawing converted from a canvas (value: the canvas path).
pub const CONVERTED_FROM: &str = "shale-converted-from";

const FONT_SIZE: f64 = 16.0;
const LINE_HEIGHT: f64 = 1.25;
/// Card outline when the canvas gives no colour.
const PLAIN_STROKE: &str = "#868e96";

/// An Obsidian Canvas file (`*.canvas`).
pub fn is_canvas_path(p: &Path) -> bool {
    let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    crate::excalidraw_md::strip_suffix_ci(&name, ".canvas").is_some_and(|stem| !stem.is_empty())
}

/// Canvas colour (`"1"`–`"6"` preset or `#rrggbb`) → (outline, fill) in Excalidraw's palette.
fn colours(c: Option<&str>) -> (String, String) {
    let preset = |s: &str, f: &str| (s.to_string(), f.to_string());
    match c.map(str::trim) {
        Some("1") => preset("#e03131", "#ffc9c9"),
        Some("2") => preset("#f08c00", "#ffd8a8"),
        Some("3") => preset("#e8a400", "#ffec99"),
        Some("4") => preset("#2f9e44", "#b2f2bb"),
        Some("5") => preset("#0c8599", "#99e9f2"),
        Some("6") => preset("#9c36b5", "#eebefa"),
        Some(hex) if hex.starts_with('#') && crate::drawing::parse_color(hex).is_some() => (hex.to_string(), "transparent".into()),
        _ => (PLAIN_STROKE.into(), "transparent".into()),
    }
}

/// A stable seed from an id, so the drawing looks the same every time it is opened.
fn seed_of(id: &str) -> u64 {
    id.bytes().fold(1469598103934665603u64, |h, b| (h ^ b as u64).wrapping_mul(1099511628211)) % 2_000_000_000
}

fn num(v: &Value, key: &str) -> f64 {
    v.get(key).and_then(Value::as_f64).unwrap_or(0.0)
}

fn text_of<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

/// Rough text size (the drawing view measures labels again with the real font).
fn text_size(text: &str) -> (f64, f64) {
    let lines: Vec<&str> = text.split('\n').collect();
    let longest = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0) as f64;
    (longest * FONT_SIZE * 0.55, lines.len() as f64 * FONT_SIZE * LINE_HEIGHT)
}

/// An element of `kind` with the box `[x, y, width, height]`.
fn base(id: &str, kind: &str, [x, y, w, h]: [f64; 4], stroke: &str, background: &str) -> Map<String, Value> {
    let v = json!({
        "id": id, "type": kind, "x": x, "y": y, "width": w, "height": h, "angle": 0,
        "strokeColor": stroke, "backgroundColor": background, "fillStyle": "solid",
        "strokeWidth": 2, "strokeStyle": "solid", "roughness": 0, "opacity": 100,
        "seed": seed_of(id), "version": 1, "versionNonce": seed_of(&format!("{id}#")), "isDeleted": false,
        "groupIds": [], "frameId": null, "roundness": null, "boundElements": null,
        "updated": 1, "link": null, "locked": false
    });
    v.as_object().cloned().unwrap_or_default()
}

fn text_element(id: &str, text: &str, x: f64, y: f64, container: Option<&str>, background: &str) -> Map<String, Value> {
    let (w, h) = text_size(text);
    let mut t = base(id, "text", [x, y, w, h], "#1e1e1e", background);
    t.insert("strokeWidth".into(), json!(1));
    for (k, v) in [
        ("text", json!(text)),
        ("originalText", json!(text)),
        ("fontSize", json!(FONT_SIZE)),
        ("fontFamily", json!(2)),
        ("textAlign", json!(if container.is_some() { "center" } else { "left" })),
        ("verticalAlign", json!(if container.is_some() { "middle" } else { "top" })),
        ("containerId", container.map(Value::from).unwrap_or(Value::Null)),
        ("lineHeight", json!(LINE_HEIGHT)),
        ("autoResize", json!(true)),
    ] {
        t.insert(k.into(), v);
    }
    t
}

/// What a card shows, and the link it carries (Excalidraw's `link` field).
fn card_text(node: &Value) -> (String, Option<String>) {
    match text_of(node, "type").unwrap_or("") {
        "text" => (text_of(node, "text").unwrap_or("").to_string(), None),
        "file" => {
            let file = text_of(node, "file").unwrap_or("");
            let sub = text_of(node, "subpath").unwrap_or("");
            let link = format!("[[{file}{sub}]]");
            (link.clone(), Some(link))
        }
        "link" => {
            let url = text_of(node, "url").unwrap_or("").to_string();
            (url.clone(), Some(url))
        }
        "group" => (text_of(node, "label").unwrap_or("").to_string(), None),
        other => (format!("({other} card)"), None),
    }
}

/// Where a side of a card is, as a fraction of its box (Excalidraw `fixedPoint`).
fn side_point(side: Option<&str>) -> Option<[f64; 2]> {
    match side? {
        "top" => Some([0.5, 0.0]),
        "right" => Some([1.0, 0.5]),
        "bottom" => Some([0.5, 1.0]),
        "left" => Some([0.0, 0.5]),
        _ => None,
    }
}

/// No side given: the sides of the two boxes that face each other.
fn facing_sides(a: [f64; 4], b: [f64; 4]) -> ([f64; 2], [f64; 2]) {
    let (dx, dy) = ((b[0] + b[2] / 2.0) - (a[0] + a[2] / 2.0), (b[1] + b[3] / 2.0) - (a[1] + a[3] / 2.0));
    if dx.abs() >= dy.abs() {
        if dx >= 0.0 {
            ([1.0, 0.5], [0.0, 0.5])
        } else {
            ([0.0, 0.5], [1.0, 0.5])
        }
    } else if dy >= 0.0 {
        ([0.5, 1.0], [0.5, 0.0])
    } else {
        ([0.5, 0.0], [0.5, 1.0])
    }
}

fn push_bound(el: &mut Map<String, Value>, id: &str, kind: &str) {
    let mut list = el.get("boundElements").and_then(Value::as_array).cloned().unwrap_or_default();
    list.push(json!({"id": id, "type": kind}));
    el.insert("boundElements".into(), Value::Array(list));
}

/// The scene for a canvas file's text. Errors say what is wrong (shown read-only).
pub fn to_scene(text: &str) -> Result<Scene, String> {
    if text.trim().is_empty() {
        return Ok(Scene::default());
    }
    let canvas: Value = serde_json::from_str(text).map_err(|e| format!("the canvas is not valid JSON: {e}"))?;
    let nodes = canvas.get("nodes").and_then(Value::as_array).cloned().unwrap_or_default();
    let edges = canvas.get("edges").and_then(Value::as_array).cloned().unwrap_or_default();
    if !canvas.is_object() {
        return Err("the canvas is not a JSON object".into());
    }

    // groups first: they are frames behind the cards
    let mut ordered: Vec<&Value> = nodes.iter().filter(|n| text_of(n, "type") == Some("group")).collect();
    ordered.extend(nodes.iter().filter(|n| text_of(n, "type") != Some("group")));

    let mut shapes: Vec<Map<String, Value>> = Vec::new();
    let mut texts: Vec<Map<String, Value>> = Vec::new();
    let mut boxes: HashMap<String, (usize, [f64; 4])> = HashMap::new();
    for (i, node) in ordered.into_iter().enumerate() {
        let id = text_of(node, "id").map(str::to_string).unwrap_or_else(|| format!("node{i}"));
        let (x, y, w, h) = (num(node, "x"), num(node, "y"), num(node, "width").max(1.0), num(node, "height").max(1.0));
        let (stroke, fill) = colours(text_of(node, "color"));
        let group = text_of(node, "type") == Some("group");
        let mut shape = base(&id, "rectangle", [x, y, w, h], &stroke, if group { "transparent" } else { &fill });
        shape.insert("roundness".into(), json!({"type": 3}));
        let (label, link) = card_text(node);
        if let Some(l) = link {
            shape.insert("link".into(), json!(l));
        }
        if group {
            shape.insert("strokeStyle".into(), json!("dashed"));
            shape.insert("strokeWidth".into(), json!(1));
            if !label.is_empty() {
                // the group's name sits above its top-left corner, as in Obsidian
                let (_, th) = text_size(&label);
                texts.push(text_element(&format!("{id}-label"), &label, x, y - th - 4.0, None, "transparent"));
            }
        } else if !label.trim().is_empty() {
            let tid = format!("{id}-text");
            let (tw, th) = text_size(&label);
            texts.push(text_element(&tid, &label, x + (w - tw) / 2.0, y + (h - th) / 2.0, Some(&id), "transparent"));
            push_bound(&mut shape, &tid, "text");
        }
        boxes.insert(id, (shapes.len(), [x, y, w, h]));
        shapes.push(shape);
    }

    let mut arrows: Vec<Map<String, Value>> = Vec::new();
    for (i, edge) in edges.iter().enumerate() {
        let id = text_of(edge, "id").map(str::to_string).unwrap_or_else(|| format!("edge{i}"));
        let (Some(from), Some(to)) = (text_of(edge, "fromNode"), text_of(edge, "toNode")) else { continue };
        let (Some(&(fi, fb)), Some(&(ti, tb))) = (boxes.get(from), boxes.get(to)) else { continue };
        let (auto_from, auto_to) = facing_sides(fb, tb);
        let fp = side_point(text_of(edge, "fromSide")).unwrap_or(auto_from);
        let tp = side_point(text_of(edge, "toSide")).unwrap_or(auto_to);
        let start = [fb[0] + fb[2] * fp[0], fb[1] + fb[3] * fp[1]];
        let end = [tb[0] + tb[2] * tp[0], tb[1] + tb[3] * tp[1]];
        let (stroke, _) = colours(text_of(edge, "color"));
        let stroke = if stroke == PLAIN_STROKE { "#495057".to_string() } else { stroke };
        let (dx, dy) = (end[0] - start[0], end[1] - start[1]);
        let mut a = base(&id, "arrow", [start[0], start[1], dx.abs(), dy.abs()], &stroke, "transparent");
        a.insert("roundness".into(), json!({"type": 2}));
        a.insert("points".into(), json!([[0.0, 0.0], [dx, dy]]));
        a.insert("lastCommittedPoint".into(), Value::Null);
        a.insert("startBinding".into(), json!({"elementId": from, "focus": 0, "gap": 0, "fixedPoint": fp}));
        a.insert("endBinding".into(), json!({"elementId": to, "focus": 0, "gap": 0, "fixedPoint": tp}));
        let head = |end: Option<&str>, default: &str| if end.unwrap_or(default) == "arrow" { json!("arrow") } else { Value::Null };
        a.insert("startArrowhead".into(), head(text_of(edge, "fromEnd"), "none"));
        a.insert("endArrowhead".into(), head(text_of(edge, "toEnd"), "arrow"));
        push_bound(&mut shapes[fi], &id, "arrow");
        if ti != fi {
            push_bound(&mut shapes[ti], &id, "arrow");
        }
        if let Some(label) = text_of(edge, "label").filter(|l| !l.trim().is_empty()) {
            // a free text in the middle of the edge (on a white patch so the line doesn't cross it)
            let (tw, th) = text_size(label);
            let mut t = text_element(&format!("{id}-label"), label, start[0] + dx / 2.0 - tw / 2.0, start[1] + dy / 2.0 - th / 2.0, None, "#ffffff");
            t.insert("textAlign".into(), json!("center"));
            texts.push(t);
        }
        arrows.push(a);
    }

    let elements: Vec<Value> = shapes.into_iter().chain(arrows).chain(texts).map(Value::Object).collect();
    let scene = json!({
        "type": "excalidraw", "version": 2, "source": "shale (from an Obsidian canvas)",
        "elements": elements,
        "appState": {"viewBackgroundColor": "#ffffff", "gridSize": null},
        "files": {}
    });
    Scene::parse(&scene.to_string())
}

/// The front matter line that marks a drawing converted from `canvas_rel`.
fn marker(canvas_rel: &Path) -> String {
    format!("{CONVERTED_FROM}: {}", json!(crate::vault::rel_to_string(canvas_rel)))
}

/// A new `.excalidraw.md` file (Obsidian Excalidraw plugin) for the canvas, marked as converted.
pub fn converted_file(canvas_rel: &Path, canvas_text: &str) -> Result<String, String> {
    let scene = to_scene(canvas_text)?;
    let file = crate::excalidraw_md::new_file(&scene);
    let at = "excalidraw-plugin: parsed\n";
    Ok(file.replacen(at, &format!("{at}{}\n", marker(canvas_rel)), 1))
}

/// Does this drawing file's front matter say it was converted from `canvas_rel`?
pub fn is_converted_from(drawing_text: &str, canvas_rel: &Path) -> bool {
    let text = drawing_text.replace("\r\n", "\n");
    let Some(front) = text.strip_prefix("---\n").and_then(|r| r.split("\n---").next()) else { return false };
    let want = marker(canvas_rel);
    front.lines().any(|l| l.trim_end() == want)
}

/// The name of the drawing a canvas converts into: `Name.canvas` → `Name.excalidraw.md`.
pub fn drawing_name_for(canvas_rel: &Path) -> String {
    let name = canvas_rel.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let stem = crate::excalidraw_md::strip_suffix_ci(&name, ".canvas").unwrap_or(&name);
    format!("{stem}.excalidraw.md")
}

/// A drawing in the vault made from this canvas earlier, if there is one.
pub fn find_converted(vault: &crate::vault::Vault, canvas_rel: &Path) -> Option<PathBuf> {
    let mut candidates: Vec<&crate::vault::FileEntry> = vault.files.iter().filter(|f| crate::excalidraw_md::is_plugin_file(&f.rel)).collect();
    // the usual place first: next to the canvas
    candidates.sort_by_key(|f| f.rel.parent() != canvas_rel.parent());
    candidates.into_iter().map(|f| f.rel.clone()).find(|rel| crate::fsutil::read_to_string(vault.abs(rel)).is_ok_and(|t| is_converted_from(&t, canvas_rel)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drawing::Element;

    const SAMPLE: &str = r##"{
	"nodes":[
		{"id":"a1","type":"text","text":"devices","x":-440,"y":-580,"width":250,"height":60},
		{"id":"b2","type":"text","text":"camera","x":-620,"y":-99,"width":250,"height":60,"color":"4"},
		{"id":"c3","type":"file","file":"Projects/Plan.md","subpath":"#Goals","x":0,"y":0,"width":200,"height":100},
		{"id":"d4","type":"link","url":"https://example.org","x":300,"y":0,"width":200,"height":100,"color":"#123456"},
		{"id":"g5","type":"group","label":"Hardware","x":-700,"y":-650,"width":800,"height":700}
	],
	"edges":[
		{"id":"e1","fromNode":"a1","fromSide":"bottom","toNode":"b2","toSide":"top"},
		{"id":"e2","fromNode":"a1","toNode":"c3","fromEnd":"arrow","toEnd":"none","label":"getter / setter","color":"1"},
		{"id":"e3","fromNode":"a1","toNode":"missing"}
	]
}"##;

    fn el<'a>(s: &'a Scene, id: &str) -> &'a Element {
        s.elements.iter().find(|e| e.id == id).unwrap_or_else(|| panic!("no element {id}"))
    }

    // spec: DRAW-011
    #[test]
    fn canvas_cards_and_edges_become_a_scene() {
        let s = to_scene(SAMPLE).unwrap();
        // group first (behind), then cards; every card a rectangle at the canvas position
        assert_eq!(s.elements[0].id, "g5");
        let a = el(&s, "a1");
        assert_eq!((a.kind.as_str(), a.x, a.y, a.width, a.height), ("rectangle", -440.0, -580.0, 250.0, 60.0));
        // text cards get a centred label bound to the card
        let label = el(&s, "a1-text");
        assert_eq!(label.text.as_deref(), Some("devices"));
        assert_eq!(label.extra["containerId"], json!("a1"));
        assert!(a.extra["boundElements"].as_array().unwrap().iter().any(|b| b["id"] == "a1-text"));
        // colours: presets map to the palette, hex stays
        assert_eq!(el(&s, "b2").stroke_color, "#2f9e44");
        assert_eq!(el(&s, "b2").background_color, "#b2f2bb");
        assert_eq!(el(&s, "d4").stroke_color, "#123456");
        // file cards are wiki links (with the heading), link cards their address
        assert_eq!(el(&s, "c3-text").text.as_deref(), Some("[[Projects/Plan.md#Goals]]"));
        assert_eq!(el(&s, "c3").extra["link"], json!("[[Projects/Plan.md#Goals]]"));
        assert_eq!(el(&s, "d4").extra["link"], json!("https://example.org"));
        // groups: dashed frame with its name above
        assert_eq!(el(&s, "g5").stroke_style, "dashed");
        assert_eq!(el(&s, "g5-label").text.as_deref(), Some("Hardware"));
        assert!(el(&s, "g5-label").y < -650.0);
    }

    // spec: DRAW-011
    #[test]
    fn canvas_edges_attach_to_the_named_sides() {
        let s = to_scene(SAMPLE).unwrap();
        let e1 = el(&s, "e1");
        assert_eq!(e1.kind, "arrow");
        // bottom middle of a1 → top middle of b2
        assert_eq!((e1.x, e1.y), (-440.0 + 125.0, -580.0 + 60.0));
        assert_eq!(e1.points[1], [-620.0 + 125.0 - e1.x, -99.0 - e1.y]);
        assert_eq!(e1.extra["startBinding"]["fixedPoint"], json!([0.5, 1.0]));
        assert_eq!(e1.extra["endBinding"]["elementId"], json!("b2"));
        // default ends: arrow at the target only
        assert_eq!(e1.extra["startArrowhead"], Value::Null);
        assert_eq!(e1.extra["endArrowhead"], json!("arrow"));
        // no sides given: the facing sides; ends swapped; label in the middle; colour
        let e2 = el(&s, "e2");
        assert_eq!(e2.extra["startArrowhead"], json!("arrow"));
        assert_eq!(e2.extra["endArrowhead"], Value::Null);
        assert_eq!(e2.stroke_color, "#e03131");
        assert_eq!(el(&s, "e2-label").text.as_deref(), Some("getter / setter"));
        // an edge to a card that isn't there is left out
        assert!(!s.elements.iter().any(|e| e.id == "e3"));
        // the shapes list their connections (so moving them in a converted copy keeps them)
        assert!(el(&s, "b2").extra["boundElements"].as_array().unwrap().iter().any(|b| b["id"] == "e1"));
    }

    // spec: DRAW-011
    #[test]
    fn broken_and_empty_canvases() {
        assert!(to_scene("").unwrap().elements.is_empty());
        assert!(to_scene("{}").unwrap().elements.is_empty());
        assert!(to_scene("{\"nodes\": [").is_err());
        assert!(to_scene("[1, 2]").is_err());
        // unknown card types still show up
        let s = to_scene(r#"{"nodes":[{"id":"x","type":"widget","x":0,"y":0,"width":10,"height":10}]}"#).unwrap();
        assert_eq!(el(&s, "x-text").text.as_deref(), Some("(widget card)"));
        assert!(is_canvas_path(Path::new("a/Plan.canvas")) && is_canvas_path(Path::new("Plan.CANVAS")));
        assert!(!is_canvas_path(Path::new(".canvas")) && !is_canvas_path(Path::new("Plan.canvas.md")));
    }

    // spec: DRAW-012
    #[test]
    fn converted_files_are_plugin_drawings_marked_with_their_canvas() {
        let rel = Path::new("Projects/lending system.canvas");
        let text = converted_file(rel, SAMPLE).unwrap();
        assert!(text.starts_with("---\n\nexcalidraw-plugin: parsed\nshale-converted-from: \"Projects/lending system.canvas\"\n"));
        assert!(is_converted_from(&text, rel));
        assert!(!is_converted_from(&text, Path::new("Other.canvas")));
        assert!(!is_converted_from("no front matter", rel));
        // the plugin file reads back as the same drawing
        let back = crate::excalidraw_md::read(&text).unwrap();
        let shown = |s: &Scene| s.live().filter_map(|e| e.text.clone()).collect::<Vec<_>>();
        assert_eq!(shown(&back), shown(&to_scene(SAMPLE).unwrap()));
        assert_eq!(drawing_name_for(rel), "lending system.excalidraw.md");
        assert!(converted_file(rel, "{oops").is_err());
    }
}
