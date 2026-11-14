//! Pictures in drawings (Excalidraw `image` elements). The element names a `fileId`; the picture
//! itself comes from
//! - the `## Embedded Files` section of an Obsidian plugin drawing (`fileId: [[picture.png]]`,
//!   resolved like a link from the drawing; read into `Scene::embedded`), or
//! - the scene's `files` map (`dataURL: "data:image/png;base64,…"`), as excalidraw.com saves them.
//!
//! Only raster formats the `image` crate reads are shown; SVG, PDF, markdown, formulas and web
//! addresses stay a dashed box, as does a picture that can't be found.

use crate::drawing::Scene;
use crate::vault::Vault;
use eframe::egui;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Textures of a scene's pictures, by `fileId` (what the painter needs).
pub type Pictures = HashMap<String, egui::TextureId>;

/// Where one picture comes from.
enum Source {
    File(PathBuf),
    /// a `dataURL` of the scene's `files`
    Data(String),
}

/// The `fileId` of every picture the scene uses, with its source.
fn sources(scene: &Scene, vault: &Vault, rel: &Path) -> Vec<(String, Source)> {
    let mut out: Vec<(String, Source)> = Vec::new();
    let used: Vec<&str> = scene.live().filter(|e| e.kind == "image").filter_map(|e| e.extra.get("fileId").and_then(|v| v.as_str())).collect();
    for id in used {
        if out.iter().any(|(k, _)| k == id) {
            continue;
        }
        let file = scene.embedded.iter().find(|(k, _)| k == id).and_then(|(_, target)| vault.resolve(target, Some(rel))).filter(|p| crate::media::is_image(p));
        if let Some(p) = file {
            out.push((id.to_string(), Source::File(vault.abs(&p))));
        } else if let Some(url) = scene.files.get(id).and_then(|f| f.get("dataURL")).and_then(|v| v.as_str()) {
            out.push((id.to_string(), Source::Data(url.to_string())));
        }
    }
    out
}

/// Textures for the scene's pictures (loaded once, then kept by egui).
pub fn textures(ctx: &egui::Context, scene: &Scene, vault: &Vault, rel: &Path) -> Pictures {
    let mut out = Pictures::new();
    for (id, src) in sources(scene, vault, rel) {
        let tex = match src {
            Source::File(abs) => crate::media::texture(ctx, &abs),
            Source::Data(url) => {
                // the file id is the picture's content hash in Excalidraw, so it is a fine cache key
                let key = egui::Id::unique(("shale-drawing-picture", id.clone(), url.len()));
                if let Some(cached) = ctx.data(|d| d.get_temp::<Option<egui::TextureHandle>>(key)) {
                    cached
                } else {
                    let loaded = decode_data_url(&url).and_then(|b| image::load_from_memory(&b).ok()).map(|img| {
                        let img = if img.width() > 2048 || img.height() > 2048 { img.thumbnail(2048, 2048) } else { img };
                        let rgba = img.to_rgba8();
                        let color = egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw());
                        ctx.load_texture(format!("drawing-picture-{id}"), color, egui::TextureOptions::LINEAR)
                    });
                    ctx.data_mut(|d| d.insert_temp(key, loaded.clone()));
                    loaded
                }
            }
        };
        if let Some(t) = tex {
            out.insert(id, t.id());
        }
    }
    out
}

/// The scene's pictures as images (for PDF export), by `fileId`.
pub fn images(scene: &Scene, vault: &Vault, rel: &Path) -> HashMap<String, image::RgbaImage> {
    sources(scene, vault, rel)
        .into_iter()
        .filter_map(|(id, src)| {
            let img = match src {
                Source::File(abs) => image::open(abs).ok()?,
                Source::Data(url) => image::load_from_memory(&decode_data_url(&url)?).ok()?,
            };
            Some((id, img.to_rgba8()))
        })
        .collect()
}

/// The bytes of a `data:<type>;base64,<data>` address (raster pictures only).
fn decode_data_url(url: &str) -> Option<Vec<u8>> {
    let (head, data) = url.strip_prefix("data:")?.split_once(',')?;
    if !head.ends_with(";base64") || head.contains("svg") {
        return None;
    }
    base64_decode(data)
}

fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        } as u32)
    };
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.bytes().filter(|c| !c.is_ascii_whitespace() && *c != b'=') {
        acc = (acc << 6) | val(c)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// The `## Embedded Files` section of a plugin drawing: (fileId, link target) per picture.
pub fn embedded_files(text: &str) -> Vec<(String, String)> {
    let Some(start) = ["\n## Embedded Files\n", "\n# Embedded Files\n"].iter().filter_map(|h| text.find(h).map(|i| i + h.len())).min() else { return vec![] };
    let body = &text[start..];
    let end = [body.find("\n#"), body.find("\n%%")].into_iter().flatten().min().unwrap_or(body.len());
    body[..end]
        .lines()
        .filter_map(|line| {
            let (id, rest) = line.split_once(": ")?;
            let link = crate::markdown::find_links(rest).into_iter().next()?;
            (!link.target.is_empty()).then(|| (id.trim().to_string(), link.target))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 2×1 PNG: one red, one blue pixel.
    pub(crate) fn tiny_png() -> Vec<u8> {
        let img = image::RgbaImage::from_fn(2, 1, |x, _| if x == 0 { image::Rgba([255, 0, 0, 255]) } else { image::Rgba([0, 0, 255, 255]) });
        let mut bytes = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png).unwrap();
        bytes
    }

    fn base64(b: &[u8]) -> String {
        const K: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut s = String::new();
        for c in b.chunks(3) {
            let n = c.iter().enumerate().fold(0u32, |n, (i, x)| n | (*x as u32) << (16 - 8 * i));
            for i in 0..4 {
                s.push(if i <= c.len() { K[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
            }
        }
        s
    }

    // spec: DRAW-018
    #[test]
    fn data_urls_and_the_embedded_files_section() {
        let png = tiny_png();
        assert_eq!(decode_data_url(&format!("data:image/png;base64,{}", base64(&png))), Some(png.clone()));
        for odd in [&b"a"[..], b"ab", b"abc", b"abcd"] {
            assert_eq!(base64_decode(&base64(odd)).as_deref(), Some(odd));
        }
        assert_eq!(decode_data_url("data:image/svg+xml;base64,PHN2Zz4="), None, "SVG is not shown");
        assert_eq!(decode_data_url("https://example.org/a.png"), None);
        let file = "x\n## Embedded Files\nabc123: [[z_media/photo.png]]\n\ndef456: [[Plan.pdf|100%]]\n\nurl1: https://example.org/a.png\n\n%%\n## Drawing\n";
        assert_eq!(embedded_files(file), vec![("abc123".to_string(), "z_media/photo.png".to_string()), ("def456".into(), "Plan.pdf".into())]);
        assert!(embedded_files("no section").is_empty());
    }

    // spec: DRAW-018
    #[test]
    fn pictures_are_found_in_the_vault_or_the_scene() {
        let d = std::env::temp_dir().join(format!("shale-pictures-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        crate::vault::write_sample_vault(&d).unwrap();
        std::fs::write(d.join("z_media/dot.png"), tiny_png()).unwrap();
        let v = Vault::open(&d).unwrap();
        let mut scene = Scene::parse(&format!(
            r#"{{"type":"excalidraw","version":2,"elements":[
                {{"id":"a","type":"image","x":0,"y":0,"width":20,"height":10,"fileId":"f1"}},
                {{"id":"b","type":"image","x":30,"y":0,"width":20,"height":10,"fileId":"f2"}},
                {{"id":"c","type":"image","x":60,"y":0,"width":20,"height":10,"fileId":"gone"}}],
              "files":{{"f2":{{"mimeType":"image/png","id":"f2","dataURL":"data:image/png;base64,{}"}}}}}}"#,
            base64(&tiny_png())
        ))
        .unwrap();
        scene.embedded = vec![("f1".into(), "dot.png".into()), ("gone".into(), "missing.png".into())];
        let imgs = images(&scene, &v, Path::new("Sketch.excalidraw"));
        assert_eq!(imgs.len(), 2, "f1 from the vault, f2 from the data URL; the missing one is left out");
        assert_eq!(imgs["f1"].get_pixel(1, 0).0, [0, 0, 255, 255]);
        assert_eq!(imgs["f2"].get_pixel(0, 0).0, [255, 0, 0, 255]);
        let ctx = egui::Context::default();
        let tex = textures(&ctx, &scene, &v, Path::new("Sketch.excalidraw"));
        assert!(tex.contains_key("f1") && tex.contains_key("f2") && !tex.contains_key("gone"));
        // painted as a picture where there is one, as a dashed box where there is none
        let painter = egui::Painter::new(ctx.clone(), egui::LayerId::background(), egui::Rect::EVERYTHING);
        let view = crate::drawing::View { origin: egui::Pos2::ZERO, offset: egui::Vec2::ZERO, zoom: 1.0 };
        let shapes = |id: &str| {
            let mut out = Vec::new();
            crate::drawing::paint_element(&painter, scene.elements.iter().find(|e| e.id == id).unwrap(), &view, false, &tex, &mut out);
            out
        };
        assert!(matches!(shapes("a").as_slice(), [egui::Shape::Mesh(m)] if m.texture_id == tex["f1"]));
        assert!(!shapes("c").iter().any(|s| matches!(s, egui::Shape::Mesh(_))));
        // a plugin drawing reads its `## Embedded Files`
        let file = crate::excalidraw_md::new_file(&scene).replace("\n%%\n## Drawing", "\n## Embedded Files\nf1: [[dot.png]]\n\n%%\n## Drawing");
        assert_eq!(crate::excalidraw_md::read(&file).unwrap().embedded, vec![("f1".to_string(), "dot.png".to_string())]);
        let _ = std::fs::remove_dir_all(&d);
    }
}
