//! A report of what occupies the Linux release binary (BUILD-045, #697).
//!
//! `cargo run --example size_report -- --map FILE --version V --out docs/size-report.md
//! [--bin FILE] [--stripped FILE] [--root DIR] [--date YYYY-MM-DD]`
//!
//! `scripts/size-report.sh` makes the input: it links the profile `size-report` with
//! `-Wl,-Map=FILE` (lld's linker map). The map lists every input section of every object that went
//! into the binary, so each loaded byte (not the symbol table, not debug sections) belongs to the
//! crate whose object holds it. Three things need more than that:
//!
//! - the objects of `shale` itself also hold the generic code of other crates that was
//!   monomorphised for Shale's types (`Vec<shale::X>::push`): a section is named after its symbol
//!   (`.text._R…`), so the symbol is demangled and the section goes to the crate of its path;
//! - the files that `include_bytes!` and `build.rs` (the user guide) embed are big `.rodata`
//!   blobs of exactly the size of the file: they get a slice of their own;
//! - the unwind tables (`.eh_frame`, …) and the strings the linker merges are slices of their own.
//!
//! The sections are grouped (`GROUPS`) for the pie chart. The report is written as Markdown with the
//! pie as `size-pie.svg`; `size-history.tsv` keeps one row per version (a rerun for a version
//! replaces its row) and `size-history.svg` charts it. The output has no timestamp: the same input
//! gives the same report. The program needs only `std` and `rustc-demangle`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[path = "report_common/mod.rs"]
mod report_common;
use report_common::{nice_scale, spaced, write_if_changed, xml_escape};

const USAGE: &str = "usage: cargo run --example size_report -- --map FILE --version V --out FILE [--bin FILE] [--stripped FILE] [--root DIR] [--date YYYY-MM-DD]\n\n  --map FILE       the linker map (lld) of the size-report build\n  --version V      the version this report is for (a row of the history, replaced by a rerun)\n  --out FILE       write the report to FILE, and size-pie.svg, size-history.tsv and size-history.svg next to it\n  --bin FILE       the linked binary, for its file size\n  --stripped FILE  the same binary stripped, for its file size\n  --root DIR       the checkout, for the files that are embedded (default: this one)\n  --date D         the date of this version (default: unknown)";

const PIE_NAME: &str = "size-pie.svg";
const HISTORY_SVG_NAME: &str = "size-history.svg";
const TSV_NAME: &str = "size-history.tsv";
/// The first line of the TSV; a file that starts otherwise is not read.
const TSV_HEAD: &str = "# size-history";

/// Embedded files below this size are not looked for: a section of a few bytes is as likely a
/// string as a file.
const MIN_EMBEDDED: u64 = 1024;
/// What `build.rs` leaves out of the bundled guide (`NOT_BUNDLED` there).
const NOT_BUNDLED: [&str; 3] = [".shale/state.json", ".shale/tag-words.json", ".trash"];
/// Output sections that hold the unwind tables.
const UNWIND: [&str; 3] = [".eh_frame", ".eh_frame_hdr", ".gcc_except_table"];
/// Output sections of the file that are not loaded into memory.
const NOT_LOADED: [&str; 4] = [".symtab", ".strtab", ".shstrtab", ".comment"];

// ---- the groups of the pie ----

/// The labels that are no crate: what the linker or this program makes out of a section.
const K_SHALE: &str = "shale";
const K_EMBEDDED: &str = "[embedded files]";
const K_STRINGS: &str = "[merged strings]";
const K_UNWIND: &str = "[unwind data]";
const K_LINKER: &str = "[linker tables]";
const K_PADDING: &str = "[alignment padding]";
const K_SYSTEM: &str = "[system objects]";

struct Group {
    name: &'static str,
    color: &'static str,
    /// Crate names, or a prefix with a trailing `*`.
    members: &'static [&'static str],
}

/// The slices of the pie, in the order of the legend. A crate that no group names is in the last,
/// "Other".
const GROUPS: [Group; 15] = [
    Group { name: "Shale", color: "#2563eb", members: &[K_SHALE] },
    Group {
        name: "Rust std",
        color: "#64748b",
        members: &[
            "std",
            "core",
            "alloc",
            "proc_macro",
            "compiler_builtins",
            "panic_*",
            "unwind",
            "gimli",
            "object",
            "addr2line",
            "memchr",
            "miniz_oxide",
            "adler2",
            "rustc_demangle",
            "hashbrown",
            "std_detect",
            "cfg_if",
            "libc",
            "rustc_std_workspace_*",
        ],
    },
    Group {
        name: "UI",
        color: "#16a34a",
        members: &["egui*", "eframe", "epaint*", "emath", "ecolor", "winit", "accesskit*", "arboard", "smithay*", "wayland*", "x11*", "calloop*", "xkbcommon*", "raw_window_handle", "cursor_icon", "rfd", "sctk*"],
    },
    Group { name: "GPU", color: "#9333ea", members: &["wgpu*", "naga", "glow", "ash", "gpu_*", "khronos*", "renderdoc_sys", "glutin*", "metal", "d3d12"] },
    Group {
        name: "Text and fonts",
        color: "#ea580c",
        members: &[
            "skrifa",
            "read_fonts",
            "harfrust",
            "fontique",
            "parley",
            "swash",
            "ttf_parser",
            "vello*",
            "fontdb",
            "rustybuzz",
            "glifo",
            "peniko",
            "kurbo",
            "font_types",
            "fearless_simd*",
            "linebender_resource_handle",
            "cosmic_text",
            "ab_glyph*",
            "owned_ttf_parser",
            "unicode_bidi",
            "unicode_script",
            "unicode_segmentation",
        ],
    },
    Group { name: "PDF", color: "#dc2626", members: &["hayro*", "pdf_writer", "subsetter", "pic_scale", "moxcms", "flate2", "lopdf"] },
    Group {
        name: "Citations and Unicode data",
        color: "#0891b2",
        members: &["hayagriva", "citationberg", "icu*", "biblatex", "unic*", "tinystr", "zerovec*", "litemap", "writeable", "yoke*", "zerofrom*", "potential_utf", "unscanny", "yaml_*", "serde_yaml*"],
    },
    Group { name: "SQLite", color: "#ca8a04", members: &["libsqlite3_sys", "rusqlite", "libsqlite3*"] },
    Group { name: "TLS and crypto", color: "#db2777", members: &["rustls*", "ring", "webpki*", "aws*", "untrusted", "subtle", "zeroize", "sha*", "hmac", "aes*", "chacha*", "x509*", "rcgen"] },
    Group { name: "D-Bus", color: "#4f46e5", members: &["zbus*", "zvariant*", "enumflags2*", "serde_repr", "event_listener*", "async_*", "futures_*"] },
    Group { name: "Syntax highlighting", color: "#65a30d", members: &["syntect", "two_face", "fancy_regex", "onig*", "plist"] },
    Group { name: "Images, audio and regex", color: "#0d9488", members: &["image*", "png", "zune*", "jpeg*", "gif", "webp*", "tiff", "symphonia*", "rubato", "regex*", "aho_corasick"] },
    Group { name: "Embedded files", color: "#b45309", members: &[K_EMBEDDED] },
    Group { name: "Strings and unwind data", color: "#7c3aed", members: &[K_STRINGS, K_UNWIND] },
    Group { name: "Other", color: "#94a3b8", members: &[] },
];

/// The index of the group `key` (a crate name or one of the `K_` labels) belongs to.
fn group_of(key: &str) -> usize {
    let hit = |m: &str| m.strip_suffix('*').map_or(m == key, |prefix| key.starts_with(prefix));
    GROUPS.iter().position(|g| g.members.iter().any(|m| hit(m))).unwrap_or(GROUPS.len() - 1)
}

// ---- the linker map ----

/// One input section of the map: the part of an object that the linker put into an output section.
#[derive(Debug, Clone, PartialEq)]
struct InSection {
    /// The output section it is in (`.text`).
    out: String,
    /// The object, as the map writes it: a path, or `<path>.rlib(<member>)`, or `<internal>`.
    object: String,
    /// `.rodata..Lanon.<hash>.3`, `.text._R…`.
    name: String,
    size: u64,
}

/// The sections of the map: the output sections with their sizes, and the input sections in them.
#[derive(Debug, Default, PartialEq)]
struct MapData {
    outs: Vec<(String, u64)>,
    ins: Vec<InSection>,
}

/// The columns of a map line: the size and what is left of the line (the text, and the number of
/// spaces before it, which says whether it is an output section, an input section or a symbol).
/// `None` for the header and anything else that is not a line of the table.
fn split_line(line: &str) -> Option<(u64, usize, &str)> {
    let mut rest = line.trim_start();
    let mut size = 0;
    for column in 0..4 {
        let end = rest.find(' ')?;
        let token = &rest[..end];
        match column {
            0 | 1 => {
                u64::from_str_radix(token, 16).ok()?;
            }
            2 => size = u64::from_str_radix(token, 16).ok()?,
            _ => {
                token.parse::<u32>().ok()?;
            }
        }
        rest = &rest[end..];
    }
    let text = rest.trim_start();
    Some((size, rest.len() - text.len(), text))
}

/// Reads lld's map: `VMA LMA Size Align Out In Symbol`. The text of an output section starts one
/// space after the alignment, that of an input section eight further, that of a symbol again eight.
/// The symbols are not read: a section holds the symbol that it is named after.
fn parse_map<'a>(lines: impl Iterator<Item = &'a str>) -> Result<MapData, String> {
    let mut map = MapData::default();
    for line in lines {
        let Some((size, spaces, text)) = split_line(line) else { continue };
        if spaces <= 4 {
            map.outs.push((text.to_string(), size));
        } else if spaces <= 12 {
            let Some(out) = map.outs.last().map(|(name, _)| name.clone()) else { continue };
            let Some(cut) = text.rfind(":(") else { continue };
            let name = text[cut + 2..].strip_suffix(')').unwrap_or(&text[cut + 2..]);
            map.ins.push(InSection { out, object: text[..cut].to_string(), name: name.to_string(), size });
        }
    }
    if map.outs.is_empty() || map.ins.is_empty() {
        return Err("the map has no sections: is it an lld map (`-Wl,-Map=FILE`)?".into());
    }
    Ok(map)
}

/// Does the output section go into memory? The symbol table, the string table and the debug
/// sections only take room in the file.
fn is_loaded(out: &str) -> bool {
    !(NOT_LOADED.contains(&out) || out.starts_with(".debug") || out.starts_with(".zdebug"))
}

// ---- who owns a section ----

/// The crate of an object of the map, or the label of what it is: `libegui-<hash>.rlib(…)` is
/// `egui`, the objects of the program itself are `shale`, `<internal>` is made by the linker, the
/// start files and the like are system objects.
fn crate_of_object(object: &str) -> String {
    if object == "<internal>" {
        return K_LINKER.into();
    }
    let file = object.split('(').next().unwrap_or(object);
    let base = file.rsplit('/').next().unwrap_or(file);
    if base.starts_with("shale.") {
        return K_SHALE.into();
    }
    if let Some(lib) = base.strip_prefix("lib") {
        if let Some(stem) = lib.strip_suffix(".rlib") {
            // `egui-0123456789abcdef`: the hash is after the last dash, crate names have none
            return stem.rsplit_once('-').map_or(stem, |(name, _)| name).to_string();
        }
        if let Some(stem) = lib.strip_suffix(".a") {
            // C code built by a crate's build script: `libsqlite3.a`, `libring_core_0_17_14_.a`
            return stem.to_string();
        }
    }
    K_SYSTEM.into()
}

/// The mangled symbol that a section is named after (`.text._RNv…`, `.rodata._ZN…`), without the
/// `.llvm.<n>` the linker adds to local names. Sections of anything but Rust items have none.
fn section_symbol(name: &str) -> Option<&str> {
    let at = name.find("._R").or_else(|| name.find("._ZN"))? + 1;
    let symbol = &name[at..];
    Some(symbol.split(".llvm.").next().unwrap_or(symbol))
}

/// The crate a path like `core::ptr::drop_glue::<shale::X>` or `<alloc::vec::Vec<T>>::push`
/// starts with: the first name in it, after the `<`, `&`, `(` and the like.
fn crate_of_path(path: &str) -> Option<String> {
    let name: String = path.chars().skip_while(|c| !c.is_alphanumeric() && *c != '_').take_while(|c| c.is_alphanumeric() || *c == '_').collect();
    // a type that is no path (`dyn Trait`, `mut`, `const`) leaves the next word
    let name = match name.as_str() {
        "dyn" | "mut" | "const" | "unsafe" | "extern" | "fn" | "impl" => return crate_of_path(path.split_once(' ')?.1),
        _ => name,
    };
    (!name.is_empty()).then_some(name)
}

/// The crate of a section named after a Rust symbol; `None` when it has no symbol or it does not
/// demangle.
fn crate_of_symbol(section: &str) -> Option<String> {
    let symbol = rustc_demangle::try_demangle(section_symbol(section)?).ok()?;
    crate_of_path(&format!("{symbol:#}"))
}

/// The sizes of the files that the program embeds: the user guide (`docs/userguide`, what
/// `build.rs` bundles) and the targets of `include_bytes!` in `src/`. A file below `MIN_EMBEDDED`
/// is left out.
fn embedded_sizes(root: &Path) -> Result<BTreeSet<u64>, String> {
    let mut sizes = BTreeSet::new();
    let mut add = |path: &Path| -> Result<(), String> {
        let len = fs::metadata(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?.len();
        if len >= MIN_EMBEDDED {
            sizes.insert(len);
        }
        Ok(())
    };
    let guide = root.join("docs/userguide");
    let mut todo = vec![guide.clone()];
    while let Some(dir) = todo.pop() {
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        for entry in entries {
            let path = entry.map_err(|e| e.to_string())?.path();
            let rel = path.strip_prefix(&guide).unwrap_or(&path).to_string_lossy().replace('\\', "/");
            if NOT_BUNDLED.contains(&rel.as_str()) {
                continue;
            }
            if path.is_dir() {
                todo.push(path);
            } else {
                add(&path)?;
            }
        }
    }
    let mut todo = vec![root.join("src")];
    while let Some(dir) = todo.pop() {
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        for entry in entries {
            let path = entry.map_err(|e| e.to_string())?.path();
            if path.is_dir() {
                todo.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let text = fs::read_to_string(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
                for piece in text.split("include_bytes!(\"").skip(1) {
                    let target = dir.join(piece.split('"').next().unwrap_or(""));
                    if target.is_file() {
                        add(&target)?;
                    }
                }
            }
        }
    }
    Ok(sizes)
}

/// What the sections of a map add up to.
#[derive(Debug, Default, PartialEq)]
struct Totals {
    /// The loaded bytes: the sizes of the output sections.
    loaded: u64,
    /// By crate or label.
    keys: BTreeMap<String, u64>,
    /// By output section.
    outs: BTreeMap<String, u64>,
    /// The bytes in the objects of `shale` itself, and how many of them were generic code of other
    /// crates (moved to them by their symbol).
    shale_objects: u64,
    moved: u64,
}

/// Attributes every loaded byte: input sections to a crate or label, the rest of an output section
/// (alignment between its input sections) to `K_PADDING`.
fn totals(map: &MapData, embedded: &BTreeSet<u64>) -> Totals {
    let mut t = Totals::default();
    let mut in_sum: BTreeMap<&str, u64> = BTreeMap::new();
    for s in map.ins.iter().filter(|s| is_loaded(&s.out)) {
        *in_sum.entry(&s.out).or_default() += s.size;
        let owner = crate_of_object(&s.object);
        let key = if UNWIND.contains(&s.out.as_str()) {
            K_UNWIND.to_string()
        } else if owner == K_LINKER && s.name.starts_with(".rodata.") && (s.name.contains(".str") || s.name.contains(".cst")) {
            K_STRINGS.to_string()
        } else if owner != K_SHALE {
            owner
        } else {
            t.shale_objects += s.size;
            if s.name.starts_with(".rodata") && embedded.contains(&s.size) {
                K_EMBEDDED.to_string()
            } else {
                let by_symbol = crate_of_symbol(&s.name).unwrap_or_else(|| K_SHALE.to_string());
                if by_symbol != K_SHALE {
                    t.moved += s.size;
                }
                by_symbol
            }
        };
        *t.keys.entry(key).or_default() += s.size;
    }
    for (name, size) in map.outs.iter().filter(|(name, _)| is_loaded(name)) {
        t.loaded += size;
        *t.outs.entry(name.clone()).or_default() += size;
        let padding = size.saturating_sub(in_sum.get(name.as_str()).copied().unwrap_or(0));
        if padding > 0 {
            *t.keys.entry(K_PADDING.into()).or_default() += padding;
        }
    }
    t
}

/// The bytes per group, in the order of `GROUPS`.
fn group_sizes(keys: &BTreeMap<String, u64>) -> Vec<u64> {
    let mut sizes = vec![0; GROUPS.len()];
    for (key, size) in keys {
        sizes[group_of(key)] += size;
    }
    sizes
}

/// Biggest first, then by name.
fn by_size(map: &BTreeMap<String, u64>) -> Vec<(String, u64)> {
    let mut rows: Vec<(String, u64)> = map.iter().map(|(k, v)| (k.clone(), *v)).collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    rows
}

// ---- the history ----

/// The numbers of one version.
#[derive(Debug, Clone, PartialEq)]
struct HistRow {
    version: String,
    date: String,
    total: u64,
    /// In the order of `GROUPS`.
    groups: Vec<u64>,
}

/// The text of `size-history.tsv`: a marker line, a header (version, date, total, the groups) and
/// a line per version.
fn tsv_text(rows: &[HistRow]) -> String {
    let mut out = format!("{TSV_HEAD}\nversion\tdate\ttotal");
    for g in &GROUPS {
        let _ = write!(out, "\t{}", g.name);
    }
    out.push('\n');
    for r in rows {
        let _ = write!(out, "{}\t{}\t{}", r.version, r.date, r.total);
        for n in &r.groups {
            let _ = write!(out, "\t{n}");
        }
        out.push('\n');
    }
    out
}

/// The rows of a `size-history.tsv`. The columns are found by the names in the header, so a group
/// that was added or renamed later does not break the old rows (it reads as 0, a column that no
/// group has any more is left out); `None` for a file that is not one.
fn parse_tsv(text: &str) -> Option<Vec<HistRow>> {
    let mut lines = text.lines();
    if lines.next()? != TSV_HEAD {
        return None;
    }
    let header: Vec<&str> = lines.next()?.split('\t').collect();
    if header.get(..3)? != ["version", "date", "total"] {
        return None;
    }
    let mut rows = Vec::new();
    for line in lines {
        let cells: Vec<&str> = line.split('\t').collect();
        if cells.len() != header.len() {
            return None;
        }
        let mut groups = vec![0; GROUPS.len()];
        for (name, cell) in header.iter().zip(&cells).skip(3) {
            if let Some(at) = GROUPS.iter().position(|g| g.name == *name) {
                groups[at] = cell.parse().ok()?;
            }
        }
        rows.push(HistRow { version: cells[0].into(), date: cells[1].into(), total: cells[2].parse().ok()?, groups });
    }
    Some(rows)
}

/// `rows` with `row` in place of the one of the same version, or after them when there is none.
fn with_row(mut rows: Vec<HistRow>, row: HistRow) -> Vec<HistRow> {
    match rows.iter().position(|r| r.version == row.version) {
        Some(at) => rows[at] = row,
        None => rows.push(row),
    }
    rows
}

// ---- the text ----

/// `91.25 MiB`, `812.0 KiB`, `12 B`.
fn size_text(n: u64) -> String {
    const KIB: f64 = 1024.0;
    let f = n as f64;
    if f >= KIB * KIB {
        format!("{:.2} MiB", f / (KIB * KIB))
    } else if f >= KIB {
        format!("{:.1} KiB", f / KIB)
    } else {
        format!("{n} B")
    }
}

/// `12.3%`.
fn percent(part: u64, whole: u64) -> String {
    if whole == 0 {
        return "0.0%".into();
    }
    format!("{:.1}%", part as f64 * 100.0 / whole as f64)
}

/// What the report is made of.
struct Report {
    version: String,
    date: Option<String>,
    totals: Totals,
    /// The file size of the measured binary and of the stripped one.
    bin_size: Option<u64>,
    stripped_size: Option<u64>,
    history: Vec<HistRow>,
}

const TOP_CRATES: usize = 25;

fn render(r: &Report) -> String {
    let t = &r.totals;
    let groups = group_sizes(&t.keys);
    let date = r.date.as_deref().map_or(String::new(), |d| format!(" ({d})"));
    let mut out = format!("# Binary size report\n\nWhat occupies the Linux x86-64 release binary of Shale **{}**{date}: **{}** loaded (`{}` bytes). This is the build of the profile `size-report`, which keeps the symbols and has no LTO, so that every byte can be attributed to a crate (see [how it is made](#how-it-is-made-and-what-to-keep-in-mind)).", r.version, size_text(t.loaded), spaced(t.loaded as usize));
    if let Some(bin) = r.bin_size {
        let _ = write!(out, " The file is {}", size_text(bin));
        if let Some(stripped) = r.stripped_size {
            let _ = write!(out, ", {} stripped", size_text(stripped));
        }
        out.push('.');
    }
    out += " The binary that is shipped (profile `dist`: fat LTO, stripped) is smaller than this.\n\n";
    let _ = write!(out, "[![Size by group of crates]({PIE_NAME})]({PIE_NAME})\n\nThe picture opens the SVG file, where hovering a slice shows its size.\n\n## By group\n\n| Group | Size | Share |\n|---|---:|---:|\n");
    let mut order: Vec<usize> = (0..GROUPS.len()).filter(|&i| groups[i] > 0).collect();
    order.sort_by(|&a, &b| groups[b].cmp(&groups[a]).then(a.cmp(&b)));
    for i in order {
        let _ = writeln!(out, "| {} | {} | {} |", GROUPS[i].name, size_text(groups[i]), percent(groups[i], t.loaded));
    }
    let _ = write!(out, "\n## The {TOP_CRATES} biggest crates\n\nNames in brackets are no crates: what the linker makes (`[linker tables]`, `[merged strings]`), the unwind tables, files that are embedded, padding to the alignment of a section.\n\n| Crate | Size | Share | Group |\n|---|---:|---:|---|\n");
    for (key, size) in by_size(&t.keys).into_iter().take(TOP_CRATES) {
        let _ = writeln!(out, "| `{key}` | {} | {} | {} |", size_text(size), percent(size, t.loaded), GROUPS[group_of(&key)].name);
    }
    out += "\n## By section of the file\n\n| Section | Size | Share |\n|---|---:|---:|\n";
    for (name, size) in by_size(&t.outs).into_iter().filter(|(_, size)| *size >= 1024) {
        let _ = writeln!(out, "| `{name}` | {} | {} |", size_text(size), percent(size, t.loaded));
    }
    out += "\n## History\n\n";
    if r.history.is_empty() {
        out += "There is no history yet.\n";
    } else {
        let _ = write!(out, "[![Size per version]({HISTORY_SVG_NAME})]({HISTORY_SVG_NAME})\n\nOne column per version (a check tag `X.Y.Z-rc.N` or a release), stacked by group; the numbers are in `{TSV_NAME}`. A version is measured when its tag is made, so the report of a tag is in the tag's own commit.\n\n<details>\n<summary>Numbers per version</summary>\n\n");
        let shown: Vec<usize> = (0..GROUPS.len()).filter(|&i| r.history.iter().any(|h| h.groups[i] > 0)).collect();
        out += "| Version | Date | Total |";
        for &i in &shown {
            let _ = write!(out, " {} |", GROUPS[i].name);
        }
        out += "\n|---|---|---:|";
        out += &"---:|".repeat(shown.len());
        out.push('\n');
        for h in r.history.iter().rev() {
            let _ = write!(out, "| {} | {} | {} |", h.version, h.date, size_text(h.total));
            for &i in &shown {
                let _ = write!(out, " {} |", size_text(h.groups[i]));
            }
            out.push('\n');
        }
        out += "\n</details>\n";
    }
    let _ = write!(
        out,
        "\n## How it is made and what to keep in mind\n\n\
`scripts/size-report.sh` builds `cargo rustc --profile size-report --locked --bin shale -- -C link-arg=-Wl,-Map=…` and `examples/size_report.rs` reads the map of the linker (lld). `scripts/make-tag.sh` and `scripts/make-release.sh` run it for every tag (not for every landing), before the version commit.\n\n\
- **Only what is loaded counts**: the sections that are in memory when the program runs (`.text`, `.rodata`, `.eh_frame`, …). The symbol table, the string table and debug sections take room in the file only, and are left out.\n\
- **Linux only, no LTO.** The size of macOS and Windows builds is not measured. With fat LTO all crates become one object and nothing can be told apart, so this is the build without it, with the symbols; the shipped binary is smaller. The shares are what to look at, not the sizes.\n\
- **Generic code is moved to the crate it comes from.** The objects of `shale` hold {} of the loaded bytes, {} of which are code of other crates that was monomorphised for Shale's types (`Vec<shale::X>::push`, `drop_glue`): a section is named after its symbol, and the crate of the symbol's path gets it. Where a symbol does not tell (an impl on a type of Shale), the code stays with `shale`.\n\
- **Embedded files** are the user guide (`docs/userguide`, bundled by `build.rs`) and the targets of `include_bytes!`: a `.rodata` section of `shale` that has exactly the size of such a file (from {} on). A file that is embedded twice is counted twice.\n\
- **Strings**: most string literals of Rust sit in sections of their own and belong to their crate. `[merged strings]` are what the linker merged (strings of C code, constants).\n\
- **Unwind data** (`.eh_frame`, `.eh_frame_hdr`, `.gcc_except_table`) is one slice, not split by crate.\n\
- The padding that the linker adds between sections for their alignment is `[alignment padding]`.\n",
        size_text(t.shale_objects),
        size_text(t.moved),
        size_text(MIN_EMBEDDED),
    );
    out
}

// ---- the charts ----

const SVG_FONT: &str = "<style>\ntext{font-family:sans-serif;font-size:12px;fill:#374151}.lbl{font-size:11px}.slice:hover path,.slice:hover rect,.col:hover rect.bar{opacity:0.75}\n</style>\n";

fn svg_head(w: f64, h: f64, label: &str) -> String {
    let mut out = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {w} {h}\" width=\"{w}\" height=\"{h}\" role=\"img\" aria-label=\"{0}\">\n<title>{0}</title>\n", xml_escape(label));
    out += SVG_FONT;
    let _ = writeln!(out, "<rect width=\"{w}\" height=\"{h}\" fill=\"#ffffff\"/>");
    out
}

/// The pie chart of the groups, with a legend: a slice per group that has bytes, biggest first;
/// each has a `<title>` with its size, and the legend entry too.
fn pie_svg(groups: &[u64], total: u64, version: &str) -> String {
    const W: f64 = 760.0;
    const H: f64 = 400.0;
    const CX: f64 = 190.0;
    const CY: f64 = 210.0;
    const R: f64 = 160.0;
    let mut out = svg_head(W, H, &format!("Binary size by group of crates, Shale {version}"));
    let _ = writeln!(out, "<text x=\"20\" y=\"28\" style=\"font-size:15px;font-weight:bold\">Shale {} · {} loaded</text>", xml_escape(version), size_text(total));
    let mut order: Vec<usize> = (0..groups.len()).filter(|&i| groups[i] > 0).collect();
    order.sort_by(|&a, &b| groups[b].cmp(&groups[a]).then(a.cmp(&b)));
    let mut angle = -std::f64::consts::FRAC_PI_2;
    for (row, &i) in order.iter().enumerate() {
        let label = format!("{}: {} ({})", GROUPS[i].name, size_text(groups[i]), percent(groups[i], total));
        let color = GROUPS[i].color;
        let share = groups[i] as f64 / total.max(1) as f64;
        let _ = writeln!(out, "<g class=\"slice\">\n<title>{}</title>", xml_escape(&label));
        if share >= 0.9999 {
            let _ = writeln!(out, "<path d=\"M {CX} {} A {R} {R} 0 1 1 {CX} {} A {R} {R} 0 1 1 {CX} {} Z\" fill=\"{color}\" stroke=\"#ffffff\"/>", CY - R, CY + R, CY - R);
        } else {
            let end = angle + share * std::f64::consts::TAU;
            let (x0, y0, x1, y1) = (CX + R * angle.cos(), CY + R * angle.sin(), CX + R * end.cos(), CY + R * end.sin());
            let large = u8::from(share > 0.5);
            let _ = writeln!(out, "<path d=\"M {CX} {CY} L {x0:.2} {y0:.2} A {R} {R} 0 {large} 1 {x1:.2} {y1:.2} Z\" fill=\"{color}\" stroke=\"#ffffff\"/>");
            angle = end;
        }
        let ly = 60.0 + 20.0 * row as f64;
        let _ = writeln!(out, "<rect x=\"400\" y=\"{}\" width=\"12\" height=\"12\" fill=\"{color}\"/>", ly - 10.0);
        let _ = writeln!(out, "<text x=\"420\" y=\"{ly}\">{} · {} · {}</text>\n</g>", xml_escape(GROUPS[i].name), size_text(groups[i]), percent(groups[i], total));
    }
    out + "</svg>\n"
}

/// The chart of the history: a stacked column per version, the groups in the colours of the pie,
/// the total in MiB on the y axis; hovering a column shows its numbers (`<title>`).
fn history_svg(rows: &[HistRow]) -> String {
    const W: f64 = 960.0;
    const H: f64 = 440.0;
    const LEFT: f64 = 64.0;
    const RIGHT: f64 = W - 24.0;
    const TOP: f64 = 64.0;
    const BOTTOM: f64 = H - 80.0;
    const MIB: f64 = 1024.0 * 1024.0;
    let mut out = svg_head(W, H, "Binary size per version");
    let peak = rows.iter().map(|r| r.total).max().unwrap_or(0);
    let (step, top) = nice_scale(((peak as f64 / MIB).ceil() as usize).max(1));
    let y = |mib: f64| BOTTOM - (BOTTOM - TOP) * mib / top as f64;
    for k in 0..=top / step {
        let (v, gy) = (k * step, y((k * step) as f64));
        let _ = writeln!(out, "<line x1=\"{LEFT}\" y1=\"{gy:.1}\" x2=\"{RIGHT}\" y2=\"{gy:.1}\" stroke=\"#e5e7eb\"/>");
        let _ = writeln!(out, "<text x=\"{}\" y=\"{:.1}\" text-anchor=\"end\">{v}</text>", LEFT - 8.0, gy + 4.0);
    }
    let _ = writeln!(out, "<text x=\"{LEFT}\" y=\"20\" style=\"font-weight:bold\">MiB loaded, by group</text>");
    let _ = writeln!(out, "<line x1=\"{LEFT}\" y1=\"{BOTTOM}\" x2=\"{RIGHT}\" y2=\"{BOTTOM}\" stroke=\"#6b7280\"/>");
    let used: Vec<usize> = (0..GROUPS.len()).filter(|&i| rows.iter().any(|r| r.groups[i] > 0)).collect();
    for (k, &i) in used.iter().enumerate() {
        let (lx, ly) = (LEFT + 150.0 * (k % 6) as f64, 34.0 + 14.0 * (k / 6) as f64);
        let _ = writeln!(out, "<rect x=\"{lx}\" y=\"{}\" width=\"10\" height=\"10\" fill=\"{}\"/>\n<text class=\"lbl\" x=\"{}\" y=\"{ly}\">{}</text>", ly - 9.0, GROUPS[i].color, lx + 14.0, xml_escape(GROUPS[i].name));
    }
    let n = rows.len().max(1);
    let slot = (RIGHT - LEFT) / n as f64;
    let bar = (slot * 0.6).min(48.0);
    for (c, r) in rows.iter().enumerate() {
        let x = LEFT + slot * (c as f64 + 0.5);
        let mut lines = vec![format!("{} · {} · {}", r.version, r.date, size_text(r.total))];
        lines.extend(used.iter().filter(|&&i| r.groups[i] > 0).map(|&i| format!("{}: {}", GROUPS[i].name, size_text(r.groups[i]))));
        let _ = writeln!(out, "<g class=\"col\">\n<title>{}</title>", xml_escape(&lines.join("\n")));
        let mut base = 0.0;
        for &i in &used {
            let height = r.groups[i] as f64 / MIB;
            if height > 0.0 {
                let _ = writeln!(out, "<rect class=\"bar\" x=\"{:.1}\" y=\"{:.1}\" width=\"{bar:.1}\" height=\"{:.1}\" fill=\"{}\"/>", x - bar / 2.0, y(base + height), y(base) - y(base + height), GROUPS[i].color);
            }
            base += height;
        }
        let _ = writeln!(out, "<text class=\"lbl\" transform=\"translate({:.1},{}) rotate(-45)\" text-anchor=\"end\">{}</text>\n</g>", x + 4.0, BOTTOM + 16.0, xml_escape(&r.version));
    }
    out + "</svg>\n"
}

// ---- the program ----

#[derive(Debug, PartialEq)]
struct Args {
    map: PathBuf,
    version: String,
    out: PathBuf,
    bin: Option<PathBuf>,
    stripped: Option<PathBuf>,
    root: PathBuf,
    date: Option<String>,
}

fn parse_args(args: &[String], default_root: &Path) -> Result<Args, String> {
    let (mut map, mut version, mut out, mut bin, mut stripped, mut root, mut date) = (None, None, None, None, None, None, None);
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let value = |it: &mut std::slice::Iter<String>| it.next().cloned().ok_or_else(|| format!("{arg} needs a value\n{USAGE}"));
        match arg.as_str() {
            "--map" => map = Some(PathBuf::from(value(&mut it)?)),
            "--version" => version = Some(value(&mut it)?),
            "--out" => out = Some(PathBuf::from(value(&mut it)?)),
            "--bin" => bin = Some(PathBuf::from(value(&mut it)?)),
            "--stripped" => stripped = Some(PathBuf::from(value(&mut it)?)),
            "--root" => root = Some(PathBuf::from(value(&mut it)?)),
            "--date" => date = Some(value(&mut it)?),
            _ => return Err(format!("unknown argument `{arg}`\n{USAGE}")),
        }
    }
    Ok(Args {
        map: map.ok_or_else(|| format!("--map is missing\n{USAGE}"))?,
        version: version.ok_or_else(|| format!("--version is missing\n{USAGE}"))?,
        out: out.ok_or_else(|| format!("--out is missing\n{USAGE}"))?,
        bin,
        stripped,
        root: root.unwrap_or_else(|| default_root.to_path_buf()),
        date,
    })
}

fn file_size(path: &Option<PathBuf>) -> Result<Option<u64>, String> {
    path.as_ref().map(|p| fs::metadata(p).map(|m| m.len()).map_err(|e| format!("cannot read {}: {e}", p.display()))).transpose()
}

fn execute(args: &[String], default_root: &Path) -> Result<(), String> {
    let args = parse_args(args, default_root)?;
    let text = fs::read_to_string(&args.map).map_err(|e| format!("cannot read the map {}: {e}", args.map.display()))?;
    let map = parse_map(text.lines())?;
    drop(text);
    let t = totals(&map, &embedded_sizes(&args.root)?);
    let dir = args.out.parent().unwrap_or(Path::new("")).to_path_buf();
    let old = fs::read_to_string(dir.join(TSV_NAME)).ok().and_then(|text| parse_tsv(&text)).unwrap_or_default();
    let row = HistRow { version: args.version.clone(), date: args.date.clone().unwrap_or_default(), total: t.loaded, groups: group_sizes(&t.keys) };
    let history = with_row(old, row);
    let pie = pie_svg(&group_sizes(&t.keys), t.loaded, &args.version);
    let report = Report { version: args.version, date: args.date, totals: t, bin_size: file_size(&args.bin)?, stripped_size: file_size(&args.stripped)?, history };
    write_if_changed(&args.out, &render(&report))?;
    write_if_changed(&dir.join(PIE_NAME), &pie)?;
    write_if_changed(&dir.join(TSV_NAME), &tsv_text(&report.history))?;
    write_if_changed(&dir.join(HISTORY_SVG_NAME), &history_svg(&report.history))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match execute(&args, Path::new(env!("CARGO_MANIFEST_DIR"))) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("size_report: {message}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A temporary folder with files, removed when dropped.
    struct Tmp(PathBuf);

    impl Tmp {
        fn new(files: &[(&str, &[u8])]) -> Tmp {
            static N: AtomicUsize = AtomicUsize::new(0);
            let dir = std::env::temp_dir().join(format!("size_report_{}_{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
            for (rel, bytes) in files {
                let path = dir.join(rel);
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(path, bytes).unwrap();
            }
            fs::create_dir_all(&dir).unwrap();
            Tmp(dir)
        }
    }

    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    const P: &str = "/w/target/size-report";

    /// A map line: an output section (`kind` 0), an input section (1) or a symbol (2), with the
    /// columns where lld puts them.
    fn line(kind: usize, size: u64, text: &str) -> String {
        format!("{:>16x} {:>16x} {:>8x} {:>5}{}{}", 0x1000, 0x1000, size, 4, " ".repeat([1, 9, 17][kind]), text)
    }

    /// A small map in lld's format: the header, `.text` and `.rodata` with objects of several
    /// kinds, unwind data, and sections that are not loaded.
    fn fixture() -> String {
        let rlib = |c: &str| format!("{P}/deps/lib{c}-0123456789abcdef.rlib({c}-0123456789abcdef.{c}.aaaa-cgu.0.rcgu.o)");
        let shale = format!("{P}/build/shale/12980c8f469cb4a3/out/shale.shale.86af3f6b41294b6-cgu.00.rcgu.o");
        let mut rows = vec![
            "             VMA              LMA     Size Align Out     In      Symbol".to_string(),
            line(0, 0x1000, ".text"),
            // a function of Shale, one of core monomorphised for Shale's type, one of egui
            line(1, 0x100, &format!("{shale}:(.text._RNvCs1_5shale4main)")),
            line(2, 0x100, "shale::main"),
            line(1, 0x300, &format!("{shale}:(.text._RINvNtCs2_4core3ptr9drop_glueNtCs3_5shale5ThingE)")),
            line(1, 0x400, &format!("{}:(.text._RNvCs4_4egui3run)", rlib("egui"))),
            line(1, 0x80, "/usr/lib/crt1.o:(.text)"),
            line(0, 0x2000, ".rodata"),
            line(1, 0x40, "<internal>:(.rodata.str1.1)"),
            line(1, 0x1000, &format!("{shale}:(.rodata..Lanon.f055c6b4.10)")),
            line(1, 0x200, &format!("{shale}:(.rodata..Lanon.f055c6b4.11)")),
            line(1, 0x400, &format!("{}:(.rodata..Lanon.bb4e.0)", rlib("wgpu_core"))),
            line(0, 0x800, ".eh_frame"),
            line(1, 0x700, &format!("{}:(.eh_frame+0x18)", rlib("egui"))),
            line(0, 0x500, ".symtab"),
            line(1, 0x500, "<internal>:(.symtab)"),
            line(0, 0x300, ".debug_info"),
            line(1, 0x300, &format!("{}:(.debug_info)", rlib("std"))),
        ];
        rows.push(String::new());
        rows.join("\n")
    }

    // spec: BUILD-045
    #[test]
    fn build_045_the_map_is_read_into_output_and_input_sections() {
        let text = fixture();
        let map = parse_map(text.lines()).unwrap();
        assert_eq!(map.outs.iter().map(|(n, s)| (n.as_str(), *s)).collect::<Vec<_>>(), [(".text", 0x1000), (".rodata", 0x2000), (".eh_frame", 0x800), (".symtab", 0x500), (".debug_info", 0x300)]);
        // the symbol line is no input section
        assert_eq!(map.ins.len(), 12);
        let first = &map.ins[0];
        assert_eq!((first.out.as_str(), first.name.as_str(), first.size), (".text", ".text._RNvCs1_5shale4main", 0x100));
        assert!(first.object.ends_with("-cgu.00.rcgu.o"), "{}", first.object);
        assert_eq!(map.ins.iter().find(|s| s.name == ".rodata.str1.1").unwrap().object, "<internal>");
    }

    // spec: BUILD-045
    #[test]
    fn build_045_a_file_that_is_no_map_is_refused() {
        assert!(parse_map("hello\nworld\n".lines()).unwrap_err().contains("lld map"));
        assert!(parse_map("".lines()).is_err());
        assert!(parse_map(fixture().lines().take(1)).is_err());
    }

    // spec: BUILD-045
    #[test]
    fn build_045_a_path_with_a_space_and_a_member_of_an_archive_are_read() {
        let text = line(0, 0x10, ".rodata") + "\n" + &line(1, 0x10, "/home/a b/target/libring_core_0_17_14_.a(x.o):(.rodata.k)") + "\n";
        let map = parse_map(text.lines()).unwrap();
        assert_eq!(map.ins[0].object, "/home/a b/target/libring_core_0_17_14_.a(x.o)");
        assert_eq!(crate_of_object(&map.ins[0].object), "ring_core_0_17_14_");
    }

    // spec: BUILD-045
    #[test]
    fn build_045_the_objects_belong_to_their_crates() {
        let rlib = "/w/deps/libwgpu_core-0123456789abcdef.rlib(wgpu_core-0123456789abcdef.wgpu_core.aaaa-cgu.3.rcgu.o)";
        assert_eq!(crate_of_object(rlib), "wgpu_core");
        assert_eq!(crate_of_object("/rustlib/x86_64-unknown-linux-gnu/lib/libstd-9a8b7c6d5e4f3a2b.rlib(std-9a8b7c6d5e4f3a2b.std.x-cgu.0.rcgu.o)"), "std");
        assert_eq!(crate_of_object("/w/build/shale/1/out/shale.shale.86af3f6b41294b6-cgu.00.rcgu.o"), K_SHALE);
        assert_eq!(crate_of_object("/w/deps/shale.akk4lkryhb56vj6ch91q5f9fj.rcgu.o"), K_SHALE);
        assert_eq!(crate_of_object("/w/build/libsqlite3-sys/out/libsqlite3.a(sqlite3.o)"), "sqlite3");
        assert_eq!(crate_of_object("<internal>"), K_LINKER);
        assert_eq!(crate_of_object("/usr/lib/gcc/x86_64-pc-linux-gnu/16/Scrt1.o"), K_SYSTEM);
    }

    // spec: BUILD-045
    #[test]
    fn build_045_a_section_is_named_after_its_symbol_and_the_symbol_has_a_crate() {
        assert_eq!(section_symbol(".text._RNvCs1_5shale4main"), Some("_RNvCs1_5shale4main"));
        assert_eq!(section_symbol(".rodata._RNvCs1_5shale4main.llvm.1035808941"), Some("_RNvCs1_5shale4main"));
        assert_eq!(section_symbol(".text.unlikely._ZN4core9panicking5panic17h0123456789abcdefE"), Some("_ZN4core9panicking5panic17h0123456789abcdefE"));
        assert_eq!(section_symbol(".rodata..Lanon.f055c6b4.10"), None);
        assert_eq!(crate_of_symbol(".text._RNvCs1_5shale4main").as_deref(), Some("shale"));
        assert_eq!(crate_of_symbol(".text._RINvNtCs2_4core3ptr9drop_glueNtCs3_5shale5ThingE").as_deref(), Some("core"));
        assert_eq!(crate_of_symbol(".text._ZN4core9panicking5panic17h0123456789abcdefE").as_deref(), Some("core"));
        assert_eq!(crate_of_symbol(".text._Rnotasymbol"), None);
        assert_eq!(crate_of_symbol(".text"), None);
    }

    // spec: BUILD-045
    #[test]
    fn build_045_the_first_name_of_a_path_is_its_crate() {
        let of = |p: &str| crate_of_path(p);
        assert_eq!(of("core::ptr::drop_glue::<shale::X>").as_deref(), Some("core"));
        assert_eq!(of("<alloc::vec::Vec<shale::X>>::push").as_deref(), Some("alloc"));
        assert_eq!(of("<shale::X as core::fmt::Debug>::fmt").as_deref(), Some("shale"));
        assert_eq!(of("<&mut dyn core::fmt::Write>::write_str").as_deref(), Some("core"));
        assert_eq!(of("<(u8, shale::X) as std::hash::Hash>::hash").as_deref(), Some("u8"));
        assert_eq!(of("<*const u8>::add"), Some("u8".into()));
        assert_eq!(of("<>"), None);
    }

    // spec: BUILD-045
    #[test]
    fn build_045_the_bytes_of_the_objects_of_shale_are_split_by_symbol() {
        let text = fixture();
        let t = totals(&parse_map(text.lines()).unwrap(), &BTreeSet::new());
        // `main` stays with shale; the drop glue of core for Shale's type goes to core
        assert_eq!(t.keys.get("core"), Some(&0x300));
        assert_eq!(t.moved, 0x300);
        assert_eq!(t.keys.get("egui"), Some(&0x400));
        assert_eq!(t.shale_objects, 0x100 + 0x300 + 0x1000 + 0x200);
        assert_eq!(t.keys.get(K_SHALE), Some(&(0x100 + 0x1000 + 0x200)));
    }

    // spec: BUILD-045
    #[test]
    fn build_045_a_rodata_blob_of_the_size_of_an_embedded_file_is_an_embedded_file() {
        let text = fixture();
        let t = totals(&parse_map(text.lines()).unwrap(), &BTreeSet::from([0x1000, 0x9999]));
        assert_eq!(t.keys.get(K_EMBEDDED), Some(&0x1000));
        assert_eq!(t.keys.get(K_SHALE), Some(&(0x100 + 0x200)));
        // every copy is counted, and a blob of another crate with that size is not an embedded file
        let two = format!(
            "{}\n{}\n{}\n{}\n",
            line(0, 0x2000, ".rodata"),
            line(1, 0x1000, "/w/shale.shale.a-cgu.0.rcgu.o:(.rodata..Lanon.a.1)"),
            line(1, 0x1000, "/w/shale.shale.a-cgu.0.rcgu.o:(.rodata.anon.b.2.llvm.5)"),
            line(1, 0x1000, "/w/deps/libegui-0123456789abcdef.rlib(m.o):(.rodata..Lanon.c.3)")
        );
        let t = totals(&parse_map(two.lines()).unwrap(), &BTreeSet::from([0x1000]));
        assert_eq!((t.keys.get(K_EMBEDDED), t.keys.get("egui")), (Some(&0x2000), Some(&0x1000)));
    }

    // spec: BUILD-045
    #[test]
    fn build_045_only_loaded_sections_count_and_the_rest_of_a_section_is_padding() {
        let text = fixture();
        let t = totals(&parse_map(text.lines()).unwrap(), &BTreeSet::new());
        assert_eq!(t.loaded, 0x1000 + 0x2000 + 0x800);
        assert!(!t.outs.contains_key(".symtab") && !t.outs.contains_key(".debug_info"), "{:?}", t.outs);
        assert!(!is_loaded(".debug_str") && !is_loaded(".strtab") && !is_loaded(".comment") && is_loaded(".tbss") && is_loaded(".rela.dyn"));
        // .text: 0x1000 in the output section, 0x100 + 0x300 + 0x400 + 0x80 in objects
        // .rodata: 0x40 + 0x1000 + 0x200 + 0x400 in objects; .eh_frame: 0x700 in objects
        assert_eq!(t.keys.get(K_PADDING), Some(&((0x1000 - 0x880) + (0x2000 - 0x1640) + (0x800 - 0x700))));
        assert_eq!(t.keys.values().sum::<u64>(), t.loaded, "every loaded byte has an owner");
        assert_eq!(t.keys.get(K_STRINGS), Some(&0x40));
        assert_eq!(t.keys.get(K_UNWIND), Some(&0x700));
        assert_eq!(t.keys.get(K_SYSTEM), Some(&0x80));
    }

    // spec: BUILD-045
    #[test]
    fn build_045_the_crates_are_grouped() {
        let name = |key: &str| GROUPS[group_of(key)].name;
        assert_eq!([name("shale"), name("core"), name("egui_winit"), name("wgpu_core"), name("skrifa")], ["Shale", "Rust std", "UI", "GPU", "Text and fonts"]);
        assert_eq!([name("hayro_font"), name("hayagriva"), name("icu_collator"), name("libsqlite3_sys"), name("rustls")], ["PDF", "Citations and Unicode data", "Citations and Unicode data", "SQLite", "TLS and crypto"]);
        assert_eq!([name("zbus"), name("two_face"), name("png"), name("regex_syntax"), name(K_EMBEDDED)], ["D-Bus", "Syntax highlighting", "Images, audio and regex", "Images, audio and regex", "Embedded files"]);
        assert_eq!([name(K_STRINGS), name(K_UNWIND), name("some_new_crate"), name(K_LINKER), name(K_PADDING)], ["Strings and unwind data", "Strings and unwind data", "Other", "Other", "Other"]);
        assert!(GROUPS.len() <= 15 && GROUPS.last().unwrap().name == "Other");
        let colors: BTreeSet<&str> = GROUPS.iter().map(|g| g.color).collect();
        assert_eq!(colors.len(), GROUPS.len(), "every group has a colour of its own");
    }

    // spec: BUILD-045
    #[test]
    fn build_045_the_embedded_files_are_the_guide_and_the_include_bytes_targets() {
        let big = vec![7u8; 2000];
        let tmp = Tmp::new(&[
            ("docs/userguide/a.md", &big),
            ("docs/userguide/sub/b.png", &vec![1u8; 3000]),
            ("docs/userguide/tiny.md", b"small"),
            ("docs/userguide/.shale/state.json", &vec![2u8; 4000]),
            ("docs/userguide/.trash", &vec![3u8; 5000]),
            ("assets/font.otf", &vec![4u8; 6000]),
            ("src/main.rs", b"const F: &[u8] = include_bytes!(\"../assets/font.otf\");\nconst G: &[u8] = include_bytes!(\"../assets/missing.bin\");\n"),
        ]);
        let sizes = embedded_sizes(&tmp.0).unwrap();
        assert_eq!(sizes, BTreeSet::from([2000, 3000, 6000]));
    }

    // spec: BUILD-045
    #[test]
    fn build_045_the_history_is_kept_by_version_and_a_rerun_replaces_its_row() {
        let row = |v: &str, total: u64, shale: u64| {
            let mut groups = vec![0; GROUPS.len()];
            groups[0] = shale;
            HistRow { version: v.into(), date: "2026-11-25".into(), total, groups }
        };
        let rows = with_row(Vec::new(), row("0.8.0-rc.1", 100, 10));
        let rows = with_row(rows, row("0.8.0-rc.2", 200, 20));
        let rows = with_row(rows, row("0.8.0-rc.1", 150, 15));
        assert_eq!(rows.iter().map(|r| (r.version.as_str(), r.total)).collect::<Vec<_>>(), [("0.8.0-rc.1", 150), ("0.8.0-rc.2", 200)]);
        let text = tsv_text(&rows);
        assert!(text.starts_with("# size-history\nversion\tdate\ttotal\tShale\tRust std\t"), "{text}");
        assert_eq!(parse_tsv(&text), Some(rows));
        // a group that was added later reads as 0 in old rows; one that is gone is dropped; garbage is refused
        let old = "# size-history\nversion\tdate\ttotal\tShale\tGone\n0.7.0\t2026-01-01\t90\t9\t1\n";
        let read = parse_tsv(old).unwrap();
        assert_eq!((read[0].groups[0], read[0].groups.len(), read[0].total), (9, GROUPS.len(), 90));
        assert_eq!(parse_tsv("version\tdate\ttotal\n"), None);
        assert_eq!(parse_tsv("# size-history\nversion\tdate\ttotal\tShale\n0.7.0\t2026-01-01\t90\n"), None);
        assert_eq!(parse_tsv("# size-history\nversion\tdate\ttotal\nv\td\tx\n"), None);
    }

    fn report() -> Report {
        let text = fixture();
        let totals = totals(&parse_map(text.lines()).unwrap(), &BTreeSet::from([0x1000]));
        let history = with_row(Vec::new(), HistRow { version: "0.8.0".into(), date: "2026-11-25".into(), total: totals.loaded, groups: group_sizes(&totals.keys) });
        Report { version: "0.8.0".into(), date: Some("2026-11-25".into()), totals, bin_size: Some(5 * 1024 * 1024), stripped_size: Some(4 * 1024 * 1024), history }
    }

    // spec: BUILD-045
    #[test]
    fn build_045_the_report_has_the_tables_the_charts_and_the_caveats() {
        let md = render(&report());
        assert!(md.starts_with("# Binary size report\n"), "{md}");
        for needle in [
            "**0.8.0**",
            "(2026-11-25)",
            "**14.00 KiB** loaded",
            "5.00 MiB, 4.00 MiB stripped",
            "![Size by group of crates](size-pie.svg)",
            "## By group",
            "| Embedded files | 4.0 KiB |",
            "| `egui` | 1.0 KiB |",
            "## The 25 biggest crates",
            "## By section of the file",
            "| `.text` | 4.0 KiB |",
            "![Size per version](size-history.svg)",
            "| 0.8.0 | 2026-11-25 |",
            "Linux only, no LTO",
            "monomorphised",
            "[alignment padding]",
        ] {
            assert!(md.contains(needle), "{needle} is missing in\n{md}");
        }
        assert!(!md.contains("| `.symtab`") && !md.contains(".debug_info"));
        assert_eq!(md, render(&report()), "the same input gives the same report");
        // a report without the sizes of the files and a date says nothing about them
        let mut bare = report();
        (bare.bin_size, bare.stripped_size, bare.date) = (None, None, None);
        let md = render(&bare);
        assert!(!md.contains("The file is") && !md.contains("()"));
    }

    // spec: BUILD-045
    #[test]
    fn build_045_the_pie_has_a_slice_and_a_legend_entry_per_group_with_bytes() {
        let r = report();
        let groups = group_sizes(&r.totals.keys);
        let svg = pie_svg(&groups, r.totals.loaded, "0.8.0");
        assert!(svg.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\"") && svg.trim_end().ends_with("</svg>"));
        assert!(!svg.contains("<script") && svg.contains("<rect width=\"760\" height=\"400\" fill=\"#ffffff\"/>"));
        let used = groups.iter().filter(|&&n| n > 0).count();
        assert!(used >= 4);
        assert_eq!(svg.matches("<path ").count(), used);
        assert_eq!(svg.matches("<g class=\"slice\">").count(), used);
        assert!(svg.contains("<title>Embedded files: 4.0 KiB (") && svg.contains("Shale 0.8.0 · 14.00 KiB loaded"));
        // one group is a whole disc, none is nothing but the title
        let mut one = vec![0; GROUPS.len()];
        one[2] = 500;
        let svg = pie_svg(&one, 500, "x<y");
        assert_eq!(svg.matches("<path ").count(), 1);
        assert!(svg.contains("A 160 160 0 1 1") && svg.contains("x&lt;y") && !svg.contains("x<y"));
        assert_eq!(pie_svg(&vec![0; GROUPS.len()], 0, "0").matches("<path ").count(), 0);
    }

    // spec: BUILD-045
    #[test]
    fn build_045_the_history_chart_has_a_stacked_column_per_version() {
        let r = report();
        let mut second = r.history[0].clone();
        second.version = "0.9.0-rc.1".into();
        second.total *= 2;
        second.groups = second.groups.iter().map(|n| n * 2).collect();
        let rows = vec![r.history[0].clone(), second];
        let svg = history_svg(&rows);
        assert!(svg.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\"") && svg.trim_end().ends_with("</svg>"));
        assert_eq!(svg.matches("<g class=\"col\">").count(), 2);
        let used = rows[0].groups.iter().filter(|&&n| n > 0).count();
        assert_eq!(svg.matches("<rect class=\"bar\"").count(), 2 * used);
        assert!(svg.contains("0.9.0-rc.1") && svg.contains("<title>0.8.0 · 2026-11-25 · 14.00 KiB"));
        // the empty history is a chart with axes only, and a name is escaped
        let empty = history_svg(&[]);
        assert!(empty.contains("MiB loaded") && !empty.contains("class=\"bar\""));
        let mut odd = rows[0].clone();
        odd.version = "a&b".into();
        assert!(history_svg(&[odd]).contains("a&amp;b"));
    }

    // spec: BUILD-045
    #[test]
    fn build_045_sizes_and_shares_are_written_short() {
        assert_eq!([0, 1023, 1024, 1536, 1024 * 1024, 95_713_500].map(size_text), ["0 B", "1023 B", "1.0 KiB", "1.5 KiB", "1.00 MiB", "91.28 MiB"]);
        assert_eq!([percent(1, 4), percent(0, 0), percent(1, 3)], ["25.0%", "0.0%", "33.3%"]);
    }

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    // spec: BUILD-045
    #[test]
    fn build_045_the_arguments_are_read_and_a_missing_one_is_named() {
        let a = parse_args(&args(&["--map", "m", "--version", "1.0.0", "--out", "d/r.md", "--date", "2026-01-02", "--bin", "b", "--stripped", "s"]), Path::new("/repo")).unwrap();
        assert_eq!((a.root.as_path(), a.date.as_deref(), a.bin.as_deref(), a.stripped.as_deref()), (Path::new("/repo"), Some("2026-01-02"), Some(Path::new("b")), Some(Path::new("s"))));
        assert!(parse_args(&args(&["--map", "m", "--out", "o"]), Path::new("/")).unwrap_err().contains("--version is missing"));
        assert!(parse_args(&args(&["--version", "1", "--out", "o"]), Path::new("/")).unwrap_err().contains("--map is missing"));
        assert!(parse_args(&args(&["--bogus"]), Path::new("/")).unwrap_err().contains("unknown argument"));
        assert!(parse_args(&args(&["--map"]), Path::new("/")).unwrap_err().contains("needs a value"));
    }

    // spec: BUILD-045
    #[test]
    fn build_045_the_program_writes_the_report_the_charts_and_the_history_and_a_rerun_replaces_a_row() {
        let tmp = Tmp::new(&[("map.txt", fixture().as_bytes()), ("bin", &[0u8; 2048]), ("docs/.keep", b""), ("root/src/main.rs", b"fn main() {}\n")]);
        let (map, out, root) = (tmp.0.join("map.txt"), tmp.0.join("docs/size-report.md"), tmp.0.join("root"));
        let run = |version: &str, date: &str| {
            let a = args(&["--map", map.to_str().unwrap(), "--version", version, "--date", date, "--out", out.to_str().unwrap(), "--bin", tmp.0.join("bin").to_str().unwrap(), "--root", root.to_str().unwrap()]);
            execute(&a, &tmp.0).unwrap();
        };
        run("0.8.0-rc.1", "2026-11-01");
        run("0.8.0-rc.2", "2026-11-02");
        run("0.8.0-rc.1", "2026-11-03");
        let docs = tmp.0.join("docs");
        let tsv = fs::read_to_string(docs.join(TSV_NAME)).unwrap();
        let rows = parse_tsv(&tsv).unwrap();
        assert_eq!(rows.iter().map(|r| (r.version.as_str(), r.date.as_str())).collect::<Vec<_>>(), [("0.8.0-rc.1", "2026-11-03"), ("0.8.0-rc.2", "2026-11-02")]);
        let md = fs::read_to_string(&out).unwrap();
        assert!(md.contains("**0.8.0-rc.1**") && md.contains("| 0.8.0-rc.2 | 2026-11-02 |") && md.contains("The file is 2.0 KiB."), "{md}");
        assert!(fs::read_to_string(docs.join(PIE_NAME)).unwrap().contains("Shale 0.8.0-rc.1"));
        assert_eq!(fs::read_to_string(docs.join(HISTORY_SVG_NAME)).unwrap().matches("<g class=\"col\">").count(), 2);
        // a map that cannot be read, or is none, is an error that names it
        let bad = args(&["--map", "/nonexistent/map", "--version", "1", "--out", out.to_str().unwrap()]);
        assert!(execute(&bad, &tmp.0).unwrap_err().contains("cannot read the map"));
        let bad = args(&["--map", map.to_str().unwrap(), "--version", "1", "--out", out.to_str().unwrap(), "--bin", "/nonexistent/bin", "--root", root.to_str().unwrap()]);
        assert!(execute(&bad, &tmp.0).unwrap_err().contains("cannot read /nonexistent/bin"));
    }
}
