//! The front matter of a page (SITE-023…SITE-026): the title, the tags, the properties of the
//! note, with the keys Hugo acts on moved out of its way. Written as JSON, which is YAML too, so no
//! value ever needs YAML's quoting rules.

use crate::markdown;
use serde_json::{Map, Value};
use std::path::Path;

/// The keys Hugo reserves (SITE-024): a note's value of one is written under `shale` (`shale.date`,
/// `shale.type`, …) where Hugo does not act on it.
const RESERVED: [&str; 24] = [
    "date",
    "publishdate",
    "expirydate",
    "lastmod",
    "draft",
    "type",
    "layout",
    "url",
    "slug",
    "weight",
    "aliases",
    "headless",
    "build",
    "cascade",
    "outputs",
    "markup",
    "menu",
    "menus",
    "sitemap",
    "resources",
    "translationkey",
    "lang",
    "params",
    "iscjklanguage",
];

/// Properties that are not written at all: the page's title and tags have their own rules, and the
/// note's own `publish:` has done its work.
const DROPPED: [&str; 5] = ["publish", "title", "tags", "tag", "shale"];

/// The title of a page (SITE-021, SITE-026): the `title:` property, else the title line (the file
/// name without `.md` and without its `YYMMDD_` date prefix).
pub fn page_title(rel: &Path, text: &str) -> String {
    let own = markdown::front_matter_values(text).into_iter().find(|(k, _)| k == "title").and_then(|(_, v)| v.into_iter().map(|t| t.trim().to_string()).find(|t| !t.is_empty()));
    own.unwrap_or_else(|| crate::vault::title_of(&rel.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()).to_string())
}

/// What the front matter of a page is made of.
pub struct Page<'a> {
    pub title: &'a str,
    pub tags: &'a [String],
    /// the last change of the note, as Hugo's `lastmod` (the index page lists the latest, SITE-043)
    pub lastmod: Option<String>,
}

/// The front matter of a page for the note `text`. `clean` is applied to the front matter of the
/// note before it is read: it takes the links out (a link to a note that is not published must
/// leave no trace, SITE-010).
pub fn front_matter(page: &Page, text: &str, clean: &dyn Fn(&str) -> String) -> String {
    let mut out = Map::new();
    out.insert("title".into(), Value::String(page.title.to_string()));
    if !page.tags.is_empty() {
        out.insert("tags".into(), Value::Array(page.tags.iter().map(|t| Value::String(t.clone())).collect()));
    }
    if let Some(l) = &page.lastmod {
        out.insert("lastmod".into(), Value::String(l.clone()));
    }
    let mut shale = Map::new();
    let block: String = text.split_inclusive('\n').take(markdown::front_matter_lines(text)).collect();
    for (key, values) in markdown::front_matter_values(&clean(&block)) {
        if DROPPED.contains(&key.as_str()) {
            continue;
        }
        let values: Vec<Value> = values.into_iter().map(Value::String).collect();
        let value = if values.len() == 1 {
            values.into_iter().next().unwrap_or_default()
        } else if values.is_empty() {
            Value::String(String::new())
        } else {
            Value::Array(values)
        };
        if RESERVED.contains(&key.as_str()) {
            shale.insert(key, value);
        } else {
            out.insert(key, value);
        }
    }
    if !shale.is_empty() {
        out.insert("shale".into(), Value::Object(shale));
    }
    format!("---\n{}\n---\n", serde_json::to_string_pretty(&Value::Object(out)).unwrap_or_default())
}
