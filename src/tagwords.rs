//! Tags for imported things, without any server: plain counting over the vault's notes.
//!
//! - **Direct**: a tag whose words appear in the text ("lecture-hall-b" in "… Lecture Hall B …",
//!   or the last part of a nested tag) is set right away.
//! - **Learned**: for each tag, the words that appear in most of its notes but rarely in other
//!   notes. A text with several of those words gets the tag suggested. The word lists are learned
//!   again in the background from time to time and kept in `<vault>/.shale/tag-words.json`.
//!
//! Everything is deterministic: the same notes always give the same lists.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Words that say nothing about a topic (German and English).
const STOP: &[&str] = &[
    "aber",
    "alle",
    "allem",
    "allen",
    "aller",
    "alles",
    "also",
    "anbei",
    "andere",
    "anderen",
    "auch",
    "beim",
    "bereits",
    "bitte",
    "dann",
    "dass",
    "daher",
    "damit",
    "danke",
    "dank",
    "darf",
    "darum",
    "dass",
    "deine",
    "denen",
    "denn",
    "diese",
    "diesem",
    "diesen",
    "dieser",
    "dieses",
    "doch",
    "dort",
    "durch",
    "eine",
    "einem",
    "einen",
    "einer",
    "eines",
    "etwa",
    "etwas",
    "euch",
    "eure",
    "fuer",
    "für",
    "gerne",
    "gibt",
    "grüße",
    "gruesse",
    "grüßen",
    "haben",
    "habe",
    "hallo",
    "hast",
    "hatte",
    "hier",
    "ihre",
    "ihrem",
    "ihren",
    "ihrer",
    "immer",
    "jetzt",
    "kann",
    "kein",
    "keine",
    "können",
    "koennen",
    "könnte",
    "liebe",
    "lieber",
    "machen",
    "mehr",
    "meine",
    "meinen",
    "mich",
    "mit",
    "muss",
    "nach",
    "nicht",
    "noch",
    "nur",
    "oder",
    "ohne",
    "schon",
    "sehr",
    "sein",
    "seine",
    "sich",
    "sind",
    "sowie",
    "über",
    "ueber",
    "unsere",
    "unter",
    "viele",
    "viel",
    "vielen",
    "von",
    "vom",
    "wann",
    "warum",
    "weil",
    "wenn",
    "werde",
    "werden",
    "wieder",
    "wird",
    "wir",
    "wurde",
    "zum",
    "zur",
    "zwischen",
    "about",
    "after",
    "again",
    "also",
    "best",
    "could",
    "dear",
    "does",
    "from",
    "have",
    "hello",
    "here",
    "into",
    "just",
    "like",
    "make",
    "many",
    "more",
    "most",
    "much",
    "only",
    "other",
    "please",
    "regards",
    "should",
    "some",
    "such",
    "than",
    "thank",
    "thanks",
    "that",
    "their",
    "them",
    "then",
    "there",
    "these",
    "they",
    "this",
    "those",
    "very",
    "want",
    "were",
    "what",
    "when",
    "where",
    "which",
    "will",
    "with",
    "would",
    "your",
    "yours",
    "https",
    "http",
    "www",
    "mailto",
    "type",
    "email",
    "date",
    "subject",
    "tags",
    "message",
    "attachments",
    "original",
];

/// The words of a text: lower case, letters and digits (umlauts too), at least 4 characters,
/// no stop words, not only digits.
pub fn words(text: &str) -> BTreeSet<String> {
    text.split(|c: char| !c.is_alphanumeric()).filter(|w| w.chars().count() >= 4 && !w.chars().all(|c| c.is_ascii_digit())).map(|w| w.to_lowercase()).filter(|w| !STOP.contains(&w.as_str())).collect()
}

/// The text with every run of non-letters as one space, lower case, padded with spaces.
fn flat(text: &str) -> String {
    let mut out = String::from(" ");
    let mut space = true;
    for c in text.chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
            space = false;
        } else if !space {
            out.push(' ');
            space = true;
        }
    }
    if !space {
        out.push(' ');
    }
    out
}

/// Tags that are written in the text: all words of the tag (or of the last part of a nested
/// tag) next to each other, e.g. `lecture-hall-b` in "Lecture Hall B".
pub fn direct(tags: &[String], text: &str) -> Vec<String> {
    let f = flat(text);
    let mut out: Vec<String> = Vec::new();
    for t in tags {
        let last = t.rsplit('/').next().unwrap_or(t);
        let phrase = flat(last);
        let letters = phrase.chars().filter(|c| c.is_alphanumeric()).count();
        // short tags ("ab", "it") match too much by accident
        if letters < 4 || STOP.contains(&phrase.trim()) {
            continue;
        }
        if f.contains(&phrase) && !out.contains(t) {
            out.push(t.clone());
        }
    }
    out.sort();
    out
}

/// For each tag, the words typical for its notes, with a score (0..1).
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq)]
pub struct TagWords {
    pub tags: BTreeMap<String, Vec<(String, f32)>>,
    /// how many notes it was learned from
    pub notes: usize,
}

/// Learn from notes given as (their words, their tags).
pub fn learn(docs: &[(BTreeSet<String>, Vec<String>)]) -> TagWords {
    let n = docs.len();
    let mut df: BTreeMap<&str, usize> = BTreeMap::new();
    let mut per_tag: BTreeMap<&str, (usize, BTreeMap<&str, usize>)> = BTreeMap::new();
    for (ws, tags) in docs {
        for w in ws {
            *df.entry(w).or_default() += 1;
        }
        let mut seen = BTreeSet::new();
        for t in tags {
            if !seen.insert(t.as_str()) {
                continue;
            }
            let e = per_tag.entry(t).or_default();
            e.0 += 1;
            for w in ws {
                *e.1.entry(w).or_default() += 1;
            }
        }
    }
    let common = ((n as f32) * 0.3).max(3.0) as usize;
    let mut out = TagWords { tags: BTreeMap::new(), notes: n };
    for (tag, (count, tf)) in per_tag {
        if count < 2 {
            continue;
        }
        let tag_words: Vec<String> = flat(tag).split_whitespace().map(|s| s.to_string()).collect();
        let mut list: Vec<(String, f32)> = tf
            .into_iter()
            .filter(|(w, c)| *c >= 2 && !tag_words.iter().any(|t| t == w))
            .filter_map(|(w, c)| {
                let d = df[w];
                let in_tag = c as f32 / count as f32; // how many of the tag's notes have it
                let only_here = c as f32 / d as f32; // how many of the notes with it have the tag
                (in_tag >= 0.5 && only_here >= 0.6 && d <= common.max(c)).then(|| (w.to_string(), (in_tag * only_here * 100.0).round() / 100.0))
            })
            .collect();
        list.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        list.truncate(20);
        if !list.is_empty() {
            out.tags.insert(tag.to_string(), list);
        }
    }
    out
}

/// Learn from the vault's notes (tags from `#tags` and the properties).
pub fn learn_from_vault(notes: &[(String, Vec<String>)]) -> TagWords {
    let docs: Vec<(BTreeSet<String>, Vec<String>)> = notes.iter().map(|(text, tags)| (words(text), tags.clone())).collect();
    learn(&docs)
}

/// Tags whose typical words are in the text: (tag, the words that matched), best first.
pub fn suggest(tw: &TagWords, text: &str, skip: &[String]) -> Vec<(String, Vec<String>)> {
    let ws = words(text);
    let mut out: Vec<(f32, String, Vec<String>)> = Vec::new();
    for (tag, list) in &tw.tags {
        if skip.iter().any(|s| s.eq_ignore_ascii_case(tag)) {
            continue;
        }
        let hits: Vec<&(String, f32)> = list.iter().filter(|(w, _)| ws.contains(w)).collect();
        let score: f32 = hits.iter().map(|h| h.1).sum();
        if hits.len() >= 2 || score >= 0.8 {
            out.push((score, tag.clone(), hits.iter().map(|h| h.0.clone()).collect()));
        }
    }
    out.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    out.into_iter().take(8).map(|(_, t, w)| (t, w)).collect()
}

/// Where the learned lists are kept in a vault.
pub fn file_in(vault_root: &std::path::Path) -> std::path::PathBuf {
    vault_root.join(".shale").join("tag-words.json")
}

pub fn load(vault_root: &std::path::Path) -> Option<TagWords> {
    serde_json::from_str(&crate::fsutil::read_to_string(file_in(vault_root)).ok()?).ok()
}

pub fn save(vault_root: &std::path::Path, tw: &TagWords) -> std::io::Result<()> {
    let p = file_in(vault_root);
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    crate::fsutil::write_atomic(&p, serde_json::to_string_pretty(tw).unwrap_or_default().as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec: TAG-002
    #[test]
    fn words_and_direct_tags() {
        let w = words("Hallo zusammen, der Beamer im Hörsaal B ist kaputt! 2026 https://x.de Grüße");
        assert_eq!(w.into_iter().collect::<Vec<_>>(), vec!["beamer", "hörsaal", "kaputt", "zusammen"]);
        let tags = vec!["lecture-hall-b".to_string(), "projekt/hörsaal".into(), "it".into(), "beamer".into(), "status".into()];
        assert_eq!(direct(&tags, "New projector for Lecture Hall B (Hörsaal); BEAMER broken"), vec!["beamer", "lecture-hall-b", "projekt/hörsaal"]);
        assert!(direct(&tags, "lecture hall bb").is_empty(), "whole words only");
    }

    // spec: TAG-002, TAG-003, VAULT-004
    #[test]
    fn learns_typical_words_and_suggests_tags() {
        let doc = |text: &str, tags: &[&str]| (words(text), tags.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        let docs = vec![
            doc("Beamer Projektor Leinwand Hörsaal Stand", &["av-technik"]),
            doc("Projektor Leinwand Kabel HDMI", &["av-technik"]),
            doc("Leinwand Projektor Lampe getauscht", &["av-technik"]),
            doc("Mikrofon Funkstrecke Akku", &["audio"]),
            doc("Mikrofon Akku geladen Kabel", &["audio"]),
            doc("Kabel bestellt Rechnung", &[]),
            doc("Rechnung bezahlt Kabel", &[]),
        ];
        let tw = learn(&docs);
        let av: Vec<&str> = tw.tags["av-technik"].iter().map(|(w, _)| w.as_str()).collect();
        assert_eq!(av, vec!["leinwand", "projektor"], "words in most of its notes and rarely elsewhere; not 'kabel'");
        assert_eq!(tw.tags["audio"].iter().map(|(w, _)| w.as_str()).collect::<Vec<_>>(), vec!["akku", "mikrofon"]);
        assert_eq!(learn(&docs), tw, "deterministic");
        let s = suggest(&tw, "Neue Leinwand und Projektor für Raum 3", &[]);
        assert_eq!(s, vec![("av-technik".to_string(), vec!["leinwand".to_string(), "projektor".into()])]);
        assert!(suggest(&tw, "Neue Leinwand und Projektor", &["av-technik".into()]).is_empty(), "tags already set are not suggested");
        assert!(suggest(&tw, "Nur ein Kabel", &[]).is_empty());
        // kept in the vault
        let d = std::env::temp_dir().join(format!("shale-tagwords-{}", std::process::id()));
        save(&d, &tw).unwrap();
        assert_eq!(load(&d), Some(tw));
        let _ = std::fs::remove_dir_all(&d);
    }
}
