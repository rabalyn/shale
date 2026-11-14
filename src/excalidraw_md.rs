//! Drawings of the Obsidian Excalidraw plugin: `Name.excalidraw.md`.
//!
//! Such a file is markdown: front matter, a `## Text Elements` section with every text as
//! `text ^id`, and the scene in a `## Drawing` block, either as plain ```` ```json ```` or as
//! ```` ```compressed-json ```` (LZ-String, base64, split into lines of 256 characters).
//! Rules taken from the plugin (2.x):
//! - when it loads a file, the text in `## Text Elements` wins over the text in the scene;
//! - text ids are found with `\s\^(.{8})\n+`, so they must be exactly 8 characters.
//!
//! So Shale reads the scene, applies the section's texts, and on save writes the scene back in
//! the same form (compressed stays compressed), regenerates `## Text Elements` and keeps every
//! other part of the file (front matter, notes, `## Element Links`, `## Embedded Files`).

use crate::drawing::Scene;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// `name` with the ASCII suffix `suffix` (any case) cut off, if it ends with it.
pub fn strip_suffix_ci<'a>(name: &'a str, suffix: &str) -> Option<&'a str> {
    let cut = name.len().checked_sub(suffix.len())?;
    (name.is_char_boundary(cut) && name[cut..].eq_ignore_ascii_case(suffix)).then(|| &name[..cut])
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// A drawing of the Obsidian Excalidraw plugin (`*.excalidraw.md`).
pub fn is_plugin_file(p: &Path) -> bool {
    strip_suffix_ci(&file_name(p), ".excalidraw.md").is_some()
}

/// Any drawing Shale opens in the drawing editor: `*.excalidraw` or `*.excalidraw.md`.
pub fn is_drawing_path(p: &Path) -> bool {
    let n = file_name(p);
    strip_suffix_ci(&n, ".excalidraw").is_some() || strip_suffix_ci(&n, ".excalidraw.md").is_some()
}

/// The drawing's name without `.excalidraw` / `.excalidraw.md`.
pub fn drawing_stem(name: &str) -> &str {
    strip_suffix_ci(name, ".excalidraw.md").or_else(|| strip_suffix_ci(name, ".excalidraw")).unwrap_or(name)
}

// ---------------------------------------------------------------------------------------------
// The file layout

/// Where the scene data sits in the file.
struct Block {
    start: usize,
    end: usize,
    compressed: bool,
}

fn find_block(text: &str) -> Option<Block> {
    // the heading `# Drawing` or `## Drawing`, at the start of a line
    let mut from = 0;
    let head = loop {
        let i = from + text[from..].find("# Drawing\n")?;
        let line_start = text[..i].rfind('\n').map(|j| j + 1).unwrap_or(0);
        if matches!(&text[line_start..i], "" | "#") {
            break i;
        }
        from = i + 1;
    };
    let after = &text[head..];
    let (fence, compressed) = match (after.find("```compressed-json\n"), after.find("```json\n")) {
        (Some(c), Some(j)) if j < c => (j + "```json\n".len(), false),
        (Some(c), _) => (c + "```compressed-json\n".len(), true),
        (None, Some(j)) => (j + "```json\n".len(), false),
        (None, None) => return None,
    };
    let start = head + fence;
    let end = start + text[start..].find("```")?;
    Some(Block { start, end, compressed })
}

/// Where the body of `## Text Elements` is: (start, end) byte offsets.
fn text_elements_section(text: &str, block_start: usize) -> Option<(usize, usize)> {
    let head = ["\n## Text Elements\n", "\n# Text Elements\n"].iter().filter_map(|h| text.find(h).map(|i| i + h.len())).min()?;
    if head > block_start {
        return None;
    }
    // the section ends at the next part the plugin writes, or at the drawing (with its `%%`)
    let drawing_head = text[..block_start].rfind("# Drawing\n").map(|i| text[..i].rfind('\n').map(|j| j + 1).unwrap_or(0)).unwrap_or(block_start);
    let mut end = drawing_head;
    if text[head..end].ends_with("%%\n") {
        end -= 3;
    }
    for marker in ["\n## Element Links\n", "\n# Element Links\n", "\n## Embedded Files\n", "\n# Embedded Files\n"] {
        if let Some(i) = text[head..end].find(marker) {
            end = end.min(head + i + 1);
        }
    }
    Some((head, end.max(head)))
}

/// The texts of `## Text Elements`: id → text.
fn section_texts(body: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut start = 0;
    let mut i = 0;
    // the plugin's rule: whitespace, `^`, 8 characters, one or more line breaks
    while let Some(k) = body[i..].find('^') {
        let caret = i + k;
        i = caret + 1;
        let Some(space) = body[..caret].chars().next_back().filter(|c| c.is_whitespace()) else { continue };
        let id: String = body[caret + 1..].chars().take(8).collect();
        let after = caret + 1 + id.len();
        if id.chars().count() != 8 || !body[after..].starts_with('\n') {
            continue;
        }
        let text_end = (caret - space.len_utf8()).max(start);
        let text = body[start..text_end].trim_start_matches('\n').to_string();
        out.insert(id, text);
        start = after + body[after..].len() - body[after..].trim_start_matches('\n').len();
        i = start;
    }
    out
}

fn scene_text(scene: &Value) -> Option<&str> {
    scene.get("rawText").or_else(|| scene.get("originalText")).or_else(|| scene.get("text")).and_then(|v| v.as_str())
}

/// Read a plugin drawing. Errors say what is wrong, so the editor can show it (read-only).
pub fn read(text: &str) -> Result<Scene, String> {
    let text = &text.replace("\r\n", "\n");
    let block = find_block(text).ok_or("this .excalidraw.md file has no drawing data (no `## Drawing` block)")?;
    let data = &text[block.start..block.end];
    let json = if block.compressed {
        lz_string::decompress_from_base64(data).ok_or("the compressed drawing data could not be unpacked")?
    } else {
        data.to_string()
    };
    let mut value: Value = serde_json::from_str(&json).map_err(|e| format!("the drawing data is not valid: {e}"))?;
    // the texts in `## Text Elements` win (they may have been edited as markdown)
    if let Some((s, e)) = text_elements_section(text, block.start) {
        let texts = section_texts(&text[s..e]);
        if let Some(elements) = value.get_mut("elements").and_then(|v| v.as_array_mut()) {
            for el in elements.iter_mut().filter(|el| el.get("type").and_then(|t| t.as_str()) == Some("text")) {
                let Some(id) = el.get("id").and_then(|v| v.as_str()).map(str::to_string) else { continue };
                let Some(new) = texts.get(&id) else { continue };
                if scene_text(el) == Some(new.as_str()) {
                    continue;
                }
                let obj = el.as_object_mut().expect("element is an object");
                if obj.contains_key("rawText") {
                    obj.insert("rawText".into(), Value::String(new.clone()));
                }
                obj.insert("originalText".into(), Value::String(new.clone()));
                obj.insert("text".into(), Value::String(new.clone()));
            }
        }
    }
    serde_json::from_value(value).map_err(|e| format!("the drawing data is not valid: {e}"))
}

/// A stable 8-character id (letters and digits) for an id the plugin can't use.
fn short_id(old: &str, taken: &HashSet<String>) -> String {
    const A: &[u8] = b"1234567890abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let mut salt = 0u64;
    loop {
        let mut h: u64 = 0xcbf29ce484222325 ^ salt;
        for b in old.bytes() {
            h = (h ^ b as u64).wrapping_mul(0x100000001b3);
        }
        let id: String = (0..8).map(|i| A[((h >> (i * 7)) % A.len() as u64) as usize] as char).collect();
        if !taken.contains(&id) {
            return id;
        }
        salt += 1;
    }
}

fn plugin_id_ok(id: &str) -> bool {
    id.len() == 8 && id.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// Give text elements and linked elements 8-character ids, changing every reference to them.
fn fix_ids(value: &mut Value) {
    let Some(elements) = value.get("elements").and_then(|v| v.as_array()) else { return };
    let taken: HashSet<String> = elements.iter().filter_map(|e| e.get("id").and_then(|v| v.as_str()).map(str::to_string)).collect();
    let mut map: HashMap<String, String> = HashMap::new();
    let mut used = taken.clone();
    for e in elements {
        let Some(id) = e.get("id").and_then(|v| v.as_str()) else { continue };
        let is_text = e.get("type").and_then(|t| t.as_str()) == Some("text");
        let has_link = e.get("link").and_then(|l| l.as_str()).is_some_and(|l| !l.is_empty());
        if (is_text || has_link) && !plugin_id_ok(id) {
            let new = short_id(id, &used);
            used.insert(new.clone());
            map.insert(id.to_string(), new);
        }
    }
    if map.is_empty() {
        return;
    }
    fn walk(v: &mut Value, map: &HashMap<String, String>) {
        match v {
            Value::Object(o) => {
                for (k, x) in o.iter_mut() {
                    if matches!(k.as_str(), "id" | "containerId" | "elementId" | "frameId") {
                        if let Some(new) = x.as_str().and_then(|s| map.get(s)) {
                            *x = Value::String(new.clone());
                            continue;
                        }
                    }
                    walk(x, map);
                }
            }
            Value::Array(a) => a.iter_mut().for_each(|x| walk(x, map)),
            _ => {}
        }
    }
    if let Some(elements) = value.get_mut("elements") {
        walk(elements, &map);
    }
}

/// The `## Text Elements` body for a scene (as the plugin writes it).
fn section_for(value: &Value) -> String {
    let mut out = String::new();
    let live = value.get("elements").and_then(|v| v.as_array()).into_iter().flatten().filter(|e| !e.get("isDeleted").and_then(|d| d.as_bool()).unwrap_or(false));
    let live: Vec<&Value> = live.collect();
    for e in live.iter().filter(|e| e.get("type").and_then(|t| t.as_str()) == Some("text")) {
        let id = e.get("id").and_then(|v| v.as_str()).unwrap_or_default();
        out.push_str(&format!("{} ^{id}\n\n", scene_text(e).unwrap_or_default()));
    }
    for e in live.iter().filter(|e| e.get("type").and_then(|t| t.as_str()) != Some("text")) {
        if let Some(link) = e.get("link").and_then(|l| l.as_str()).filter(|l| !l.is_empty()) {
            let id = e.get("id").and_then(|v| v.as_str()).unwrap_or_default();
            out.push_str(&format!("{link} ^{id}\n\n"));
        }
    }
    out
}

/// Data for the drawing block: compressed like the plugin (256-character lines with a blank
/// line between them) or as plain JSON.
fn block_data(json: &str, compressed: bool) -> String {
    if !compressed {
        return format!("{json}\n");
    }
    let packed = lz_string::compress_to_base64(json);
    let lines: Vec<&str> = packed.as_bytes().chunks(256).map(|c| std::str::from_utf8(c).expect("base64 is ASCII")).collect();
    format!("{}\n", lines.join("\n\n"))
}

/// The file text for `scene`, keeping everything of `original` except the texts and the data.
pub fn write(original: &str, scene: &Scene) -> Result<String, String> {
    let original = &original.replace("\r\n", "\n");
    let mut value = serde_json::to_value(scene).map_err(|e| e.to_string())?;
    fix_ids(&mut value);
    let json = serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?;
    let block = find_block(original).ok_or("this .excalidraw.md file has no drawing data")?;
    let mut out = String::with_capacity(original.len() + json.len());
    let section = text_elements_section(original, block.start);
    match section {
        Some((s, e)) => {
            out.push_str(&original[..s]);
            out.push_str(&section_for(&value));
            out.push_str(&original[e..block.start]);
        }
        None => out.push_str(&original[..block.start]),
    }
    out.push_str(&block_data(&json, block.compressed));
    out.push_str(&original[block.end..]);
    Ok(out)
}

/// A new plugin drawing file (as version 2 of the plugin makes them), compressed.
pub fn new_file(scene: &Scene) -> String {
    let template = "---\n\nexcalidraw-plugin: parsed\ntags: [excalidraw]\n\n---\n==⚠  Switch to EXCALIDRAW VIEW in the MORE OPTIONS menu of this document. ⚠==\n\n\n# Excalidraw Data\n\n## Text Elements\n%%\n## Drawing\n```compressed-json\n```\n%%";
    write(template, scene).expect("the template has a drawing block")
}

// ---------------------------------------------------------------------------------------------
// LZ-String (https://github.com/pieroxy/lz-string, MIT), the base64 variant the plugin uses.
// Works on UTF-16 code units, like JavaScript strings.

pub mod lz_string {
    use std::collections::{HashMap, HashSet};

    const KEY: &[u8; 65] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/=";

    struct Bits {
        out: Vec<u8>,
        val: u32,
        pos: u32,
    }

    impl Bits {
        fn push(&mut self, bit: u32) {
            self.val = (self.val << 1) | bit;
            if self.pos == 5 {
                self.pos = 0;
                self.out.push(KEY[self.val as usize]);
                self.val = 0;
            } else {
                self.pos += 1;
            }
        }
        fn write(&mut self, mut value: u32, n: u32) {
            for _ in 0..n {
                self.push(value & 1);
                value >>= 1;
            }
        }
    }

    pub fn compress_to_base64(input: &str) -> String {
        let units: Vec<u16> = input.encode_utf16().collect();
        let mut dict: HashMap<Vec<u16>, u32> = HashMap::new();
        let mut to_create: HashSet<Vec<u16>> = HashSet::new();
        let mut w: Vec<u16> = Vec::new();
        let (mut enlarge_in, mut dict_size, mut num_bits) = (2u32, 3u32, 2u32);
        let mut b = Bits { out: Vec::new(), val: 0, pos: 0 };

        let emit_w = |w: &Vec<u16>, b: &mut Bits, dict: &HashMap<Vec<u16>, u32>, to_create: &mut HashSet<Vec<u16>>, enlarge_in: &mut u32, num_bits: &mut u32| {
            if to_create.contains(w) {
                if w[0] < 256 {
                    b.write(0, *num_bits);
                    b.write(w[0] as u32, 8);
                } else {
                    b.write(1, *num_bits);
                    b.write(w[0] as u32, 16);
                }
                *enlarge_in -= 1;
                if *enlarge_in == 0 {
                    *enlarge_in = 1 << *num_bits;
                    *num_bits += 1;
                }
                to_create.remove(w);
            } else {
                b.write(dict[w], *num_bits);
            }
            *enlarge_in -= 1;
            if *enlarge_in == 0 {
                *enlarge_in = 1 << *num_bits;
                *num_bits += 1;
            }
        };

        for &c in &units {
            let c = vec![c];
            if !dict.contains_key(&c) {
                dict.insert(c.clone(), dict_size);
                dict_size += 1;
                to_create.insert(c.clone());
            }
            let mut wc = w.clone();
            wc.extend_from_slice(&c);
            if dict.contains_key(&wc) {
                w = wc;
            } else {
                emit_w(&w, &mut b, &dict, &mut to_create, &mut enlarge_in, &mut num_bits);
                dict.insert(wc, dict_size);
                dict_size += 1;
                w = c;
            }
        }
        if !w.is_empty() {
            emit_w(&w, &mut b, &dict, &mut to_create, &mut enlarge_in, &mut num_bits);
        }
        b.write(2, num_bits);
        // flush the last character
        loop {
            b.val <<= 1;
            if b.pos == 5 {
                b.out.push(KEY[b.val as usize]);
                break;
            }
            b.pos += 1;
        }
        let mut s = String::from_utf8(b.out).expect("base64 is ASCII");
        while s.len() % 4 != 0 {
            s.push('=');
        }
        s
    }

    pub fn decompress_from_base64(input: &str) -> Option<String> {
        let mut rev = [255u8; 256];
        for (i, &k) in KEY.iter().enumerate() {
            rev[k as usize] = i as u8;
        }
        let vals: Vec<u32> = input.bytes().filter(|b| !b.is_ascii_whitespace()).map(|b| rev[b as usize]).map(|v| if v == 255 { u32::MAX } else { v as u32 }).collect();
        if vals.contains(&u32::MAX) {
            return None;
        }
        if vals.is_empty() {
            return Some(String::new());
        }
        let length = vals.len();
        let next = |i: usize| vals.get(i).copied().unwrap_or(0);
        let (mut val, mut position, mut index) = (next(0), 32u32, 1usize);
        let read = |n: u32, val: &mut u32, position: &mut u32, index: &mut usize| -> u32 {
            let mut bits = 0;
            for p in 0..n {
                let resb = *val & *position;
                *position >>= 1;
                if *position == 0 {
                    *position = 32;
                    *val = next(*index);
                    *index += 1;
                }
                if resb > 0 {
                    bits |= 1 << p;
                }
            }
            bits
        };
        let mut dict: Vec<Vec<u16>> = vec![vec![], vec![], vec![]];
        let (mut enlarge_in, mut num_bits) = (4u32, 3u32);
        let c = match read(2, &mut val, &mut position, &mut index) {
            0 => read(8, &mut val, &mut position, &mut index),
            1 => read(16, &mut val, &mut position, &mut index),
            _ => return Some(String::new()),
        };
        let mut w = vec![c as u16];
        dict.push(w.clone());
        let mut result: Vec<u16> = w.clone();
        loop {
            if index > length {
                return None;
            }
            let mut c = read(num_bits, &mut val, &mut position, &mut index) as usize;
            match c {
                0 | 1 => {
                    let unit = read(if c == 0 { 8 } else { 16 }, &mut val, &mut position, &mut index);
                    dict.push(vec![unit as u16]);
                    c = dict.len() - 1;
                    enlarge_in -= 1;
                }
                2 => return String::from_utf16(&result).ok(),
                _ => {}
            }
            if enlarge_in == 0 {
                enlarge_in = 1 << num_bits;
                num_bits += 1;
            }
            let entry = if c < dict.len() {
                dict[c].clone()
            } else if c == dict.len() {
                let mut e = w.clone();
                e.push(w[0]);
                e
            } else {
                return None;
            };
            result.extend_from_slice(&entry);
            let mut add = w.clone();
            add.push(entry[0]);
            dict.push(add);
            enlarge_in -= 1;
            w = entry;
            if enlarge_in == 0 {
                enlarge_in = 1 << num_bits;
                num_bits += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A drawing made with the Obsidian Excalidraw plugin 2.x (a rectangle and an ellipse,
    /// labelled "rectangle" and "circle"), compressed, exactly as the plugin saved it.
    const PLUGIN_FILE: &str = include_str!("testdata/plugin-drawing.excalidraw.md");

    /// Change a text the way the drawing editor does (text, originalText and the plugin's rawText).
    fn set_text(e: &mut crate::drawing::Element, t: &str) {
        e.text = Some(t.into());
        e.extra.insert("originalText".into(), t.into());
        if e.extra.contains_key("rawText") {
            e.extra.insert("rawText".into(), t.into());
        }
    }

    fn data_of(text: &str) -> String {
        let b = find_block(text).unwrap();
        text[b.start..b.end].chars().filter(|c| !c.is_whitespace()).collect()
    }

    // spec: DRAW-007
    #[test]
    fn lz_string_matches_the_plugin() {
        // unpacking the plugin's data and packing it again gives exactly the plugin's bytes
        let packed = data_of(PLUGIN_FILE);
        let json = lz_string::decompress_from_base64(&packed).expect("unpacks");
        assert!(json.starts_with('{') && json.contains("\"elements\""));
        assert_eq!(lz_string::compress_to_base64(&json), packed);
        for s in ["", "a", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "Hörsaal ß € 😀 \u{0301}", &"abc".repeat(5000)] {
            assert_eq!(lz_string::decompress_from_base64(&lz_string::compress_to_base64(s)).as_deref(), Some(s), "{s:.20}");
        }
        assert_eq!(lz_string::decompress_from_base64("not base64 !"), None);
    }

    // spec: DRAW-007
    #[test]
    fn plugin_files_are_drawings() {
        for (name, yes) in [("A.excalidraw.md", true), ("a b.Excalidraw.MD", true), ("A.excalidraw", true), ("A.md", false), ("excalidraw.md", false)] {
            assert_eq!(is_drawing_path(Path::new(name)), yes, "{name}");
        }
        assert!(is_plugin_file(Path::new("x/A.excalidraw.md")) && !is_plugin_file(Path::new("A.excalidraw")));
        assert_eq!(drawing_stem("Plan.excalidraw.md"), "Plan");
        assert_eq!(drawing_stem("Plan.excalidraw"), "Plan");
        let scene = read(PLUGIN_FILE).unwrap();
        let texts: Vec<_> = scene.live().filter(|e| e.kind == "text").map(|e| e.text.clone().unwrap_or_default()).collect();
        assert_eq!(texts.len(), 2);
        assert!(texts.contains(&"rectangle".to_string()) && texts.contains(&"circle".to_string()));
        assert!(scene.live().any(|e| e.kind == "rectangle") && scene.live().any(|e| e.kind == "ellipse"));
    }

    // spec: DRAW-008
    #[test]
    fn saving_keeps_the_plugin_layout() {
        let scene = read(PLUGIN_FILE).unwrap();
        // unchanged scene: same front matter, same sections, compressed again, same scene
        let out = write(PLUGIN_FILE, &scene).unwrap();
        assert!(out.starts_with("---\n\nexcalidraw-plugin: parsed\ntags: [excalidraw]\n"));
        assert!(out.contains("## Text Elements\nrectangle ^R6pddU5W\n\ncircle ^ICcJd7uc\n\n%%\n## Drawing\n```compressed-json\n"));
        assert!(out.ends_with("```\n%%"));
        assert!(data_of(&out).len() > 100 && out.lines().all(|l| l.len() <= 256));
        let again = read(&out).unwrap();
        assert_eq!(again.live().count(), scene.live().count());
        // a changed label is written into the section too (the plugin reads it from there)
        let mut s2 = scene.clone();
        let t = s2.elements.iter_mut().find(|e| e.text.as_deref() == Some("circle")).unwrap();
        set_text(t, "Kreis ⚪");
        let out = write(PLUGIN_FILE, &s2).unwrap();
        assert!(out.contains("Kreis ⚪ ^ICcJd7uc\n\n"));
        assert!(!out.contains("circle ^"));
        assert_eq!(read(&out).unwrap().live().filter(|e| e.text.as_deref() == Some("Kreis ⚪")).count(), 1);
        // a plain-JSON file stays plain JSON
        let plain = PLUGIN_FILE.replace("```compressed-json\n", "```json\n");
        let plain = plain.replace(&PLUGIN_FILE[find_block(PLUGIN_FILE).unwrap().start..find_block(PLUGIN_FILE).unwrap().end], &format!("{}\n", scene.to_json()));
        let out = write(&plain, &scene).unwrap();
        assert!(out.contains("```json\n{") && read(&out).is_ok());
    }

    // spec: DRAW-008
    #[test]
    fn section_text_wins_and_ids_get_eight_characters() {
        // text edited in Obsidian's markdown view: the section differs from the scene
        let edited = PLUGIN_FILE.replace("rectangle ^R6pddU5W", "Rechteck\nzwei Zeilen ^R6pddU5W");
        let scene = read(&edited).unwrap();
        assert!(scene.live().any(|e| e.text.as_deref() == Some("Rechteck\nzwei Zeilen")));
        // a new label made by Shale (long id, bound to a shape) gets an 8-character id everywhere
        let mut s = read(PLUGIN_FILE).unwrap();
        let shape = s.elements.iter().find(|e| e.kind == "rectangle").unwrap().id.clone();
        let mut t = s.elements.iter().find(|e| e.kind == "text").unwrap().clone();
        t.id = "a-very-long-shale-id_1".into();
        set_text(&mut t, "new");
        t.extra.insert("containerId".into(), shape.clone().into());
        s.elements.push(t);
        let out = write(PLUGIN_FILE, &s).unwrap();
        assert!(!out.contains("a-very-long-shale-id_1"));
        let back = read(&out).unwrap();
        let new = back.live().find(|e| e.text.as_deref() == Some("new")).unwrap();
        assert!(plugin_id_ok(&new.id));
        assert!(out.contains(&format!("new ^{}\n\n", new.id)));
        assert_eq!(new.extra.get("containerId").and_then(|v| v.as_str()), Some(shape.as_str()));
        // the same scene always gets the same ids (no churn between saves)
        assert_eq!(write(PLUGIN_FILE, &s).unwrap(), out);
    }

    // spec: DRAW-008, DATA-007
    #[test]
    fn broken_plugin_files_are_errors() {
        assert!(read("# just a note\n").is_err());
        assert!(read("## Drawing\n```compressed-json\n!!!\n```\n").is_err());
        assert!(read("## Drawing\n```json\n{not json\n```\n").is_err());
        let fresh = new_file(&Scene::default());
        assert!(read(&fresh).unwrap().elements.is_empty());
        assert!(fresh.contains("excalidraw-plugin: parsed") && fresh.contains("```compressed-json\n"));
    }
}
