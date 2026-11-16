// Stamps each build with its time so the app can show which build is running, and bundles the user
// guide (`docs/userguide`, DEV-020) into the program: it writes a table of (path, bytes) pairs that
// `src/userguide.rs` includes.
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// What Shale writes into a vault it opens; never bundled (the same names as `NOT_COPIED` in
/// `src/userguide.rs`, which a test in `tests/userguide_delivery.rs` compares).
const NOT_BUNDLED: [&str; 3] = [".shale/state.json", ".shale/tag-words.json", ".trash"];

fn walk(base: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir).expect("read docs/userguide").map(|e| e.expect("entry").path()).collect();
    entries.sort();
    for p in entries {
        let rel = p.strip_prefix(base).expect("below the guide").to_string_lossy().replace('\\', "/");
        if NOT_BUNDLED.contains(&rel.as_str()) {
            continue;
        }
        if p.is_dir() {
            walk(base, &p, out);
        } else {
            out.push((rel, p));
        }
    }
}

fn main() {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    println!("cargo:rustc-env=SHALE_BUILD_SECS={secs}");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=docs/userguide");

    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let guide = manifest.join("docs").join("userguide");
    let mut files = Vec::new();
    walk(&guide, &guide, &mut files);
    let mut table = String::from("&[\n");
    for (rel, path) in files {
        writeln!(table, "    ({rel:?}, include_bytes!({:?}) as &[u8]),", path.to_string_lossy()).expect("write to a String");
    }
    table.push_str("]\n");
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR")).join("userguide_files.rs");
    std::fs::write(out, table).expect("write the guide table");
}
