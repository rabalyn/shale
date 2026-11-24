//! Reading and writing the tables of an sfnt font file, and cutting a TrueType font down to the
//! glyphs a document uses (PDF-036, ADR-0040 §4). The PDF writer embeds a face as a single font
//! (a face of a `.ttc` collection is taken out), and a subset keeps every glyph id, with the
//! outlines of the unused glyphs left empty, so the character map and the widths stay as they are.

use std::collections::BTreeSet;

type Tag = [u8; 4];

fn be16(d: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(d.get(at..at + 2)?.try_into().ok()?))
}

fn be32(d: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(d.get(at..at + 4)?.try_into().ok()?))
}

/// The tables of face `index` of a font file (a collection has several, a plain font one).
pub fn tables(data: &[u8], index: u32) -> Option<Vec<(Tag, &[u8])>> {
    let at = if data.get(0..4)? == b"ttcf" {
        if index >= be32(data, 8)? {
            return None;
        }
        be32(data, 12 + 4 * index as usize)? as usize
    } else if index == 0 {
        0
    } else {
        return None;
    };
    let count = be16(data, at + 4)? as usize;
    (0..count)
        .map(|n| {
            let rec = at + 12 + 16 * n;
            let tag: Tag = data.get(rec..rec + 4)?.try_into().ok()?;
            let (offset, len) = (be32(data, rec + 8)? as usize, be32(data, rec + 12)? as usize);
            Some((tag, data.get(offset..offset.checked_add(len)?)?))
        })
        .collect()
}

fn checksum(data: &[u8]) -> u32 {
    data.chunks(4).fold(0u32, |sum, c| {
        let mut w = [0u8; 4];
        w[..c.len()].copy_from_slice(c);
        sum.wrapping_add(u32::from_be_bytes(w))
    })
}

/// A font file of these tables (TrueType outlines or CFF, by which tables there are).
pub fn build(mut tables: Vec<(Tag, Vec<u8>)>) -> Vec<u8> {
    tables.sort_by_key(|(tag, _)| *tag);
    let n = tables.len() as u16;
    let mut entry_selector = 0u16;
    while 1u32 << (entry_selector + 1) <= u32::from(n) {
        entry_selector += 1;
    }
    let search_range = 16 * (1u16 << entry_selector);
    let mut out = vec![0, 1, 0, 0];
    for v in [n, search_range, entry_selector, n * 16 - search_range] {
        out.extend(v.to_be_bytes());
    }
    let mut at = 12 + 16 * tables.len();
    for (tag, data) in &tables {
        out.extend(tag);
        out.extend(checksum(data).to_be_bytes());
        out.extend((at as u32).to_be_bytes());
        out.extend((data.len() as u32).to_be_bytes());
        at += data.len().div_ceil(4) * 4;
    }
    for (_, data) in &tables {
        out.extend(data);
        out.resize(out.len().div_ceil(4) * 4, 0);
    }
    out
}

/// The tables a PDF reader needs for a TrueType font that a simple font dictionary draws.
const KEPT: [&Tag; 12] = [b"OS/2", b"cmap", b"cvt ", b"fpgm", b"glyf", b"head", b"hhea", b"hmtx", b"loca", b"maxp", b"name", b"prep"];

/// The glyphs a composite glyph is made of, with the offsets after its header.
fn components(glyph: &[u8]) -> Vec<u16> {
    let mut out = Vec::new();
    if glyph.len() < 10 || i16::from_be_bytes([glyph[0], glyph[1]]) >= 0 {
        return out;
    }
    let mut at = 10;
    while let (Some(flags), Some(gid)) = (be16(glyph, at), be16(glyph, at + 2)) {
        out.push(gid);
        at += 4 + if flags & 0x1 != 0 { 4 } else { 2 };
        at += if flags & 0x8 != 0 {
            2
        } else if flags & 0x40 != 0 {
            4
        } else if flags & 0x80 != 0 {
            8
        } else {
            0
        };
        if flags & 0x20 == 0 {
            break;
        }
    }
    out
}

/// Whether face `index` has TrueType outlines (`glyf`), which a simple PDF font embeds as text;
/// a font with CFF outlines is drawn as outlines instead.
pub fn has_glyf(data: &[u8], index: u32) -> bool {
    tables(data, index).is_some_and(|t| t.iter().any(|(tag, _)| tag == b"glyf"))
}

/// Face `index` as a font file of its own with all its tables (a face of a collection is taken
/// out; a font that is not a collection is as it is).
pub fn whole(data: &[u8], index: u32) -> Option<Vec<u8>> {
    if data.get(0..4)? != b"ttcf" && index == 0 {
        return Some(data.to_vec());
    }
    Some(build(tables(data, index)?.into_iter().map(|(t, d)| (t, d.to_vec())).collect()))
}

/// Face `index` with the outlines of the glyphs in `used` (and what they are made of, and glyph
/// 0); all other glyphs are empty. None when the font has no TrueType outlines or is damaged.
pub fn subset(data: &[u8], index: u32, used: &BTreeSet<u16>) -> Option<Vec<u8>> {
    let all = tables(data, index)?;
    let get = |tag: &Tag| all.iter().find(|(t, _)| t == tag).map(|(_, d)| *d);
    let (head, glyf, loca, maxp) = (get(b"head")?, get(b"glyf")?, get(b"loca")?, get(b"maxp")?);
    let count = be16(maxp, 4)? as usize;
    let long = be16(head, 50)? != 0;
    let offset = |g: usize| -> Option<usize> {
        if long {
            be32(loca, 4 * g).map(|v| v as usize)
        } else {
            be16(loca, 2 * g).map(|v| v as usize * 2)
        }
    };
    let glyph = |g: usize| -> Option<&[u8]> { glyf.get(offset(g)?..offset(g + 1)?) };
    let mut keep: BTreeSet<u16> = used.iter().copied().filter(|g| (*g as usize) < count).collect();
    keep.insert(0);
    let mut todo: Vec<u16> = keep.iter().copied().collect();
    while let Some(g) = todo.pop() {
        for c in components(glyph(g as usize)?) {
            if (c as usize) < count && keep.insert(c) {
                todo.push(c);
            }
        }
    }
    let (mut new_glyf, mut new_loca) = (Vec::new(), Vec::with_capacity(4 * (count + 1)));
    for g in 0..count {
        new_loca.extend((new_glyf.len() as u32).to_be_bytes());
        if keep.contains(&(g as u16)) {
            new_glyf.extend(glyph(g)?);
            new_glyf.resize(new_glyf.len().div_ceil(4) * 4, 0);
        }
    }
    new_loca.extend((new_glyf.len() as u32).to_be_bytes());
    let mut new_head = head.to_vec();
    new_head.get_mut(50..52)?.copy_from_slice(&1u16.to_be_bytes());
    new_head.get_mut(8..12)?.copy_from_slice(&[0; 4]);
    let mut out: Vec<(Tag, Vec<u8>)> = all
        .iter()
        .filter(|(t, _)| KEPT.contains(&t))
        .map(|(t, d)| {
            let data = match t {
                b"glyf" => new_glyf.clone(),
                b"loca" => new_loca.clone(),
                b"head" => new_head.clone(),
                _ => d.to_vec(),
            };
            (*t, data)
        })
        .collect();
    // glyph names are not needed to draw: a `post` table without them
    if let Some(post) = get(b"post").filter(|p| p.len() >= 32) {
        let mut p = post[..32].to_vec();
        p[0..4].copy_from_slice(&[0, 3, 0, 0]);
        out.push((*b"post", p));
    }
    Some(build(out))
}
