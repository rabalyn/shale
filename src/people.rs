//! People: contact notes in the vault's people folder (default "Adressbuch"), finding the
//! people of an imported e-mail or event, and matching them to existing contact notes by
//! e-mail address or name.

use crate::markdown;
use crate::vault::{rel_to_string, Vault};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const DEFAULT_DIR: &str = "Adressbuch";

#[derive(Clone, Debug, PartialEq)]
pub enum Role {
    From,
    To,
    Cc,
    Organizer,
    Attendee,
}

impl Role {
    pub fn label(&self) -> &'static str {
        match self {
            Role::From => "from",
            Role::To => "to",
            Role::Cc => "cc",
            Role::Organizer => "organizer",
            Role::Attendee => "attendee",
        }
    }
}

/// One person of an imported item.
#[derive(Clone, Debug, PartialEq)]
pub struct Person {
    pub name: String,
    /// lower case, may be empty
    pub email: String,
    pub role: Role,
    /// their contact note, if there is one
    pub existing: Option<PathBuf>,
    /// link them (and make a note for them when there is none)
    pub on: bool,
}

/// "Müller, Hans" → "Hans Müller"; quotes and extra spaces removed.
pub fn clean_name(n: &str) -> String {
    let n = n.trim().trim_matches(|c| c == '"' || c == '\'').trim();
    let n = match n.split_once(',') {
        Some((last, first)) if !first.trim().is_empty() && !last.trim().is_empty() && !first.contains(',') && last.split_whitespace().count() <= 2 => format!("{} {}", first.trim(), last.trim()),
        _ => n.to_string(),
    };
    n.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A name from an address when none was given: "hans.mueller@x.de" → "Hans Mueller".
fn name_from_email(email: &str) -> String {
    let local = email.split('@').next().unwrap_or("");
    local
        .split(['.', '_', '-', '+'])
        .filter(|p| !p.is_empty() && !p.chars().all(|c| c.is_ascii_digit()))
        .map(|p| {
            let mut c = p.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// `"Müller, Hans" <h@x.de>, info@y.de, Team <t@z>` → [(name, email)].
pub fn parse_addresses(list: &str) -> Vec<(String, String)> {
    let mut parts = Vec::new();
    let mut cur = String::new();
    let (mut quoted, mut angle) = (false, false);
    for c in list.chars() {
        match c {
            '"' => quoted = !quoted,
            '<' => angle = true,
            '>' => angle = false,
            ',' | ';' if !quoted && !angle => {
                parts.push(std::mem::take(&mut cur));
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    parts.push(cur);
    parts
        .into_iter()
        .filter_map(|p| {
            let p = p.trim();
            if p.is_empty() {
                return None;
            }
            let (name, email) = match (p.rfind('<'), p.rfind('>')) {
                (Some(a), Some(b)) if b > a => (p[..a].trim().to_string(), p[a + 1..b].trim().to_string()),
                _ if p.contains('@') && !p.contains(' ') => (String::new(), p.to_string()),
                _ => (p.to_string(), String::new()),
            };
            let email = email.trim_start_matches("mailto:").to_lowercase();
            let mut name = clean_name(&name);
            if name.is_empty() || name.contains('@') {
                name = name_from_email(&email);
            }
            (!name.is_empty() || !email.is_empty()).then_some((name, email))
        })
        .collect()
}

/// Addresses that are no person (newsletters, systems).
fn is_robot(email: &str) -> bool {
    let local = email.split('@').next().unwrap_or("");
    ["noreply", "no-reply", "donotreply", "do-not-reply", "mailer-daemon", "postmaster", "notification", "notifications", "bounce"].iter().any(|r| local.contains(r))
}

/// The contact notes: by e-mail address (found anywhere in the note) and by name.
#[derive(Default, Debug)]
pub struct Contacts {
    by_email: HashMap<String, PathBuf>,
    by_name: HashMap<String, PathBuf>,
}

fn name_key(n: &str) -> String {
    clean_name(n).to_lowercase()
}

/// Words with an @ in a text, as e-mail addresses.
pub fn emails_in(text: &str) -> Vec<String> {
    text.split(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '(' | ')' | '[' | ']' | ',' | ';' | '"' | '\'' | '|'))
        .map(|w| w.trim_matches(|c: char| c == '.' || c == ':' || c == '*').trim_start_matches("mailto:"))
        .filter(|w| {
            let mut it = w.splitn(2, '@');
            let (a, b) = (it.next().unwrap_or(""), it.next().unwrap_or(""));
            !a.is_empty() && b.contains('.') && !b.starts_with('.') && !b.contains('@')
        })
        .map(|w| w.to_lowercase())
        .collect()
}

impl Contacts {
    pub fn index(vault: &Vault, dir: &str) -> Self {
        let dir = dir.trim().trim_matches('/');
        let mut c = Contacts::default();
        if dir.is_empty() {
            return c;
        }
        let prefix = format!("{}/", dir.to_lowercase());
        let mut notes: Vec<(&PathBuf, &crate::vault::NoteIndex)> = vault.notes.iter().filter(|(p, _)| rel_to_string(p).to_lowercase().starts_with(&prefix)).collect();
        notes.sort_by(|a, b| a.0.cmp(b.0));
        for (p, n) in notes {
            let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            c.by_name.entry(name_key(&stem)).or_insert_with(|| p.clone());
            for e in emails_in(&n.text) {
                c.by_email.entry(e).or_insert_with(|| p.clone());
            }
        }
        c
    }

    pub fn find(&self, name: &str, email: &str) -> Option<PathBuf> {
        (!email.is_empty()).then(|| self.by_email.get(email)).flatten().or_else(|| (!name.is_empty()).then(|| self.by_name.get(&name_key(name))).flatten()).cloned()
    }
}

/// The people of an e-mail: sender first, then To and Cc. New senders are ticked; new
/// recipients not (that is often yourself or a whole team); known contacts always are.
pub fn of_mail(m: &crate::mail::Mail, contacts: &Contacts) -> Vec<Person> {
    let mut out: Vec<Person> = Vec::new();
    for (list, role) in [(&m.from, Role::From), (&m.to, Role::To), (&m.cc, Role::Cc)] {
        for (name, email) in parse_addresses(list) {
            push(&mut out, name, email, role.clone(), contacts);
        }
    }
    out
}

/// The people of an event: organizer and attendees.
pub fn of_event(e: &crate::cal_ics::Event, contacts: &Contacts) -> Vec<Person> {
    let mut out: Vec<Person> = Vec::new();
    for (name, email) in parse_addresses(&e.organizer) {
        push(&mut out, name, email, Role::Organizer, contacts);
    }
    for a in &e.attendees {
        for (name, email) in parse_addresses(a) {
            push(&mut out, name, email, Role::Attendee, contacts);
        }
    }
    out
}

fn push(out: &mut Vec<Person>, name: String, email: String, role: Role, contacts: &Contacts) {
    if (!email.is_empty() && out.iter().any(|p| p.email == email)) || (email.is_empty() && out.iter().any(|p| name_key(&p.name) == name_key(&name))) {
        return;
    }
    if is_robot(&email) || name.is_empty() {
        return;
    }
    let existing = contacts.find(&name, &email);
    let on = existing.is_some() || matches!(role, Role::From | Role::Organizer);
    out.push(Person { name, email, role, existing, on });
}

/// Mail domains that say nothing about where someone works.
const FREE_MAIL: [&str; 16] = ["gmail.com", "googlemail.com", "gmx.de", "gmx.net", "web.de", "t-online.de", "outlook.com", "hotmail.com", "live.com", "yahoo.com", "yahoo.de", "icloud.com", "me.com", "mailbox.org", "posteo.de", "proton.me"];

/// The note for a new contact.
pub fn note_for(p: &Person) -> String {
    let domain = p.email.split('@').nth(1).unwrap_or("").to_string();
    let org = if domain.is_empty() || FREE_MAIL.contains(&domain.as_str()) { String::new() } else { domain };
    let mut s = String::from("---\ntype: person\n");
    if !p.email.is_empty() {
        s.push_str(&format!("email: {}\n", p.email));
    }
    if !org.is_empty() {
        s.push_str(&format!("organization: {org}\n"));
    }
    s.push_str("tags: [person]\n---\n\n");
    s.push_str(&format!("# {}\n\n", p.name));
    s.push_str(&format!("- **E-mail:** {}\n", p.email));
    s.push_str("- **Phone:** \n");
    s.push_str(&format!("- **Organization:** {org}\n"));
    s.push_str("- **Role / department:** \n\n## Notes\n\n");
    s
}

/// The link text for a contact note: just the name when that is unique in the vault, else the path.
pub fn link_to(vault: &Vault, rel: &Path) -> String {
    let stem = rel.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let unique = vault.resolve(&stem, None).as_deref() == Some(rel);
    if unique {
        format!("[[{stem}]]")
    } else {
        format!("[[{}|{stem}]]", markdown::strip_md_ext(&rel_to_string(rel)))
    }
}

/// Make sure the person has a contact note (made in `dir` if needed). Returns its path.
pub fn ensure_note(vault: &mut Vault, dir: &str, p: &Person) -> Result<PathBuf, String> {
    if let Some(e) = &p.existing {
        if vault.abs(e).exists() {
            return Ok(e.clone());
        }
    }
    // made a moment ago for another item of the same import?
    if let Some(e) = Contacts::index(vault, dir).find(&p.name, &p.email) {
        return Ok(e);
    }
    let name = markdown::link_safe_name(&markdown::sanitize_note_name(&p.name.replace('/', "-")));
    let rel = vault.new_note_path(&crate::import::clean_folder(dir)?, if name.is_empty() { "Unknown" } else { &name });
    vault.create_file(&rel, &note_for(p)).map_err(|e| format!("could not make the contact note for {}: {e}", p.name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_and_names() {
        let a = parse_addresses("\"Müller, Hans\" <H.Mueller@Arch.example>, info@lindner.example; Team Bau <bau@uni.example>, =?x?=, Ada Lovelace");
        assert_eq!(a, vec![
            ("Hans Müller".to_string(), "h.mueller@arch.example".to_string()),
            ("Info".into(), "info@lindner.example".into()),
            ("Team Bau".into(), "bau@uni.example".into()),
            ("=?x?=".into(), String::new()),
            ("Ada Lovelace".into(), String::new()),
        ]);
        assert_eq!(parse_addresses("<anna.schmidt@x.de>")[0].0, "Anna Schmidt");
        assert_eq!(clean_name("  Schmidt,  Anna "), "Anna Schmidt");
        assert_eq!(clean_name("Bauamt, Abteilung Technik, Haus 2"), "Bauamt, Abteilung Technik, Haus 2", "not a person's name");
        assert_eq!(emails_in("- **E-mail:** h.mueller@arch.example, (alt: <hm@x.de>) mailto:A@B.de."), vec!["h.mueller@arch.example", "hm@x.de", "a@b.de"]);
    }

    #[test]
    fn people_of_a_mail_and_their_notes() {
        let d = std::env::temp_dir().join(format!("shale-people-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("Adressbuch")).unwrap();
        std::fs::write(d.join("Adressbuch/Hans Müller.md"), "# Hans Müller\n- E-mail: other@arch.example\n").unwrap();
        std::fs::write(d.join("Adressbuch/Bauamt Info.md"), "- **E-mail:** bau@uni.example\n").unwrap();
        let mut v = Vault::open(&d).unwrap();
        v.rescan();
        let c = Contacts::index(&v, "Adressbuch/");
        let m = crate::mail::Mail { from: "\"Müller, Hans\" <h.mueller@arch.example>".into(), to: "Me <me@uni.example>, noreply@system.example".into(), cc: "Bauamt <bau@uni.example>".into(), ..Default::default() };
        let p = of_mail(&m, &c);
        let short: Vec<(&str, &str, bool, bool)> = p.iter().map(|p| (p.name.as_str(), p.role.label(), p.existing.is_some(), p.on)).collect();
        assert_eq!(short, vec![("Hans Müller", "from", true, true), ("Me", "to", false, false), ("Bauamt", "cc", true, true)], "found by name, robots left out, found by address");
        assert_eq!(p[2].existing.as_deref(), Some(Path::new("Adressbuch/Bauamt Info.md")));
        // a new person gets a note from the template, once
        let new = Person { name: "Anna Schmidt".into(), email: "anna@lindner.example".into(), role: Role::From, existing: None, on: true };
        let rel = ensure_note(&mut v, "Adressbuch", &new).unwrap();
        assert_eq!(rel, PathBuf::from("Adressbuch/Anna Schmidt.md"));
        let text = std::fs::read_to_string(v.abs(&rel)).unwrap();
        assert!(text.contains("email: anna@lindner.example\norganization: lindner.example\ntags: [person]") && text.contains("# Anna Schmidt"), "{text}");
        assert_eq!(ensure_note(&mut v, "Adressbuch", &new).unwrap(), rel, "not made twice");
        assert_eq!(link_to(&v, &rel), "[[Anna Schmidt]]");
        let free = Person { email: "x@gmail.com".into(), ..new.clone() };
        assert!(!note_for(&free).contains("organization:"));
        // events: organizer and attendees as the calendar stores them
        let e = crate::cal_ics::Event { organizer: "Hans Müller <h.mueller@arch.example>".into(), attendees: vec!["Anna Schmidt <anna@lindner.example>".into(), "Raum 2.04".into()], ..Default::default() };
        v.rescan();
        let p = of_event(&e, &Contacts::index(&v, "Adressbuch"));
        assert_eq!(p.iter().map(|p| (p.name.as_str(), p.existing.is_some(), p.on)).collect::<Vec<_>>(), vec![("Hans Müller", true, true), ("Anna Schmidt", true, true), ("Raum 2.04", false, false)]);
        let _ = std::fs::remove_dir_all(&d);
    }
}
