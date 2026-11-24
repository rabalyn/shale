//! Which notes are published (SITE-002, SITE-053…SITE-057): the publishing rules of the vault's
//! `.shale/settings.json` applied to a note. Reading only (SITE-059): nothing here writes a file.

use crate::markdown;
use crate::vault::name_key;
use crate::vault_state::{PublishSettings, VaultSettings};
use std::path::Path;

/// The note types that hold private data and that "all notes" leaves out (SITE-056).
pub const PRIVATE_TYPES: [&str; 5] = ["email", "event", "post", "social-post", "person"];

/// The private kind a type belongs to: a saved post is written as `social-post` (SOC-011) and is
/// the same kind as `post`, so one tick (and one name in `private_types`) covers both.
pub fn kind_of(t: &str) -> &str {
    if t == "social-post" {
        "post"
    } else {
        t
    }
}

/// The publishing settings of the vault at `root`, read without touching anything: a missing file
/// is all default (publishing off). A file that cannot be read is an error, so a broken settings
/// file never publishes by surprise and is never set aside by an export (SITE-016).
pub fn read_settings(root: &Path) -> Result<VaultSettings, String> {
    let path = crate::vault_state::VaultData::settings_path(root);
    match crate::fsutil::read(&path) {
        Ok(bytes) => crate::vault_state::parse::<VaultSettings>(&bytes).map(|(s, _)| s).map_err(|e| format!("{} cannot be read: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(VaultSettings::default()),
        Err(e) => Err(format!("{} cannot be read: {e}", path.display())),
    }
}

/// What the rules need to know of a note.
pub struct Candidate<'a> {
    pub rel: &'a Path,
    pub text: &'a str,
    /// every tag of the note, inline and front matter (TAG-001)
    pub tags: &'a [String],
}

/// The `publish:` property of a note: `Some(true)` or `Some(false)` when it says so (SITE-057).
pub fn publish_property(text: &str) -> Option<bool> {
    let (_, values) = markdown::front_matter_values(text).into_iter().find(|(k, _)| k == "publish")?;
    match values.first()?.trim().to_lowercase().as_str() {
        "true" | "yes" => Some(true),
        "false" | "no" => Some(false),
        _ => None,
    }
}

/// The `type:` of a note in lower case.
pub fn note_type(text: &str) -> Option<String> {
    let (_, values) = markdown::front_matter_values(text).into_iter().find(|(k, _)| k == "type")?;
    values.first().map(|v| v.trim().to_lowercase()).filter(|v| !v.is_empty())
}

/// Whether the folder rule `folder` holds `rel` (the folder itself and everything below it).
fn in_folder(rel: &Path, folder: &str) -> bool {
    let parts = |s: &str| s.split('/').filter(|p| !p.is_empty() && *p != ".").map(name_key).collect::<Vec<_>>();
    let want = parts(folder);
    let have = parts(&crate::vault::rel_to_string(rel.parent().unwrap_or(Path::new(""))));
    !want.is_empty() && have.starts_with(&want)
}

/// Whether a note tag matches the tag rule `rule` (`#a` or `a`): the same tag, or one below it, so
/// `a/b` matches a rule for `a` (SITE-054).
fn tag_matches(tag: &str, rule: &str) -> bool {
    let (tag, rule) = (name_key(tag.trim_start_matches('#')), name_key(rule.trim().trim_start_matches('#')));
    !rule.is_empty() && (tag == rule || tag.strip_prefix(&rule).is_some_and(|rest| rest.starts_with('/')))
}

fn has_tag(tags: &[String], rules: &[String]) -> bool {
    tags.iter().any(|t| rules.iter().any(|r| tag_matches(t, r)))
}

/// Whether a note is of a private type that is not ticked in the settings (SITE-056): its `type:`,
/// or a place in the people folder (PPL-001).
fn private_and_not_ticked(s: &PublishSettings, people_dir: &str, c: &Candidate) -> bool {
    let ticked = |t: &str| s.private_types.iter().any(|x| kind_of(&x.trim().to_lowercase()) == kind_of(t));
    let kind = note_type(c.text).filter(|t| PRIVATE_TYPES.contains(&t.as_str()));
    let person = !people_dir.trim_matches('/').is_empty() && in_folder(c.rel, people_dir);
    match (kind, person) {
        (Some(t), _) => !ticked(&t),
        (None, true) => !ticked("person"),
        (None, false) => false,
    }
}

/// Whether the rules choose the note (SITE-002). The switch of the vault is the caller's (SITE-051).
/// The note's own `publish:` wins over every rule (SITE-057); an exclude rule wins over every
/// include rule (SITE-055); "all notes" takes no note of a private type that is not ticked (SITE-056).
pub fn chosen(s: &PublishSettings, people_dir: &str, c: &Candidate) -> bool {
    if let Some(own) = publish_property(c.text) {
        return own;
    }
    if s.exclude_folders.iter().any(|f| in_folder(c.rel, f)) || has_tag(c.tags, &s.exclude_tags) {
        return false;
    }
    s.include_folders.iter().any(|f| in_folder(c.rel, f)) || has_tag(c.tags, &s.include_tags) || (s.all_notes && !private_and_not_ticked(s, people_dir, c))
}
