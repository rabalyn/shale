//! Conflict copies of cloud clients and of Shale itself (NET-015): the file names the clients give
//! a second version of a note, and the original a copy belongs to. Pure name parsing; the vault
//! pairs a copy with its original (`Vault::conflict_of`).

use std::path::PathBuf;

/// Who made a conflict copy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Client {
    Syncthing,
    Dropbox,
    Nextcloud,
    OwnCloud,
    Seafile,
    /// Shale's own copy (SYNC-008, DATA-003).
    Shale,
}

impl Client {
    /// The name shown to the user.
    pub fn label(self) -> &'static str {
        match self {
            Client::Syncthing => "Syncthing",
            Client::Dropbox => "Dropbox",
            Client::Nextcloud => "Nextcloud",
            Client::OwnCloud => "ownCloud",
            Client::Seafile => "Seafile",
            Client::Shale => "Shale",
        }
    }
}

/// A note that is a conflict copy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Conflict {
    pub client: Client,
    /// The note it is a copy of: the same folder, the original's file name. None when that file
    /// does not exist (the copy stays an ordinary note, with a notice).
    pub original: Option<PathBuf>,
}

/// If `file_name` is a conflict copy's name: the client and the file name of the original.
/// Strict: only `.md` files, and only the exact patterns of docs/storage-on-shares.md.
pub fn original_of(file_name: &str) -> Option<(Client, String)> {
    let lower = file_name.to_ascii_lowercase();
    let ext_len = if lower.ends_with(".excalidraw.md") {
        ".excalidraw.md".len()
    } else if lower.ends_with(".md") {
        ".md".len()
    } else {
        return None;
    };
    let (stem, ext) = file_name.split_at(file_name.len() - ext_len);
    let (client, base) = syncthing(stem).or_else(|| owncloud(stem)).or_else(|| parenthesised(stem))?;
    if base.trim().is_empty() {
        return None;
    }
    Some((client, format!("{base}{ext}")))
}

fn digits(s: &str, n: usize) -> bool {
    s.len() == n && s.bytes().all(|b| b.is_ascii_digit())
}

fn number(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// `Name.sync-conflict-YYYYMMDD-HHMMSS`, and `-DEVICE` (seven letters and digits) after it.
fn syncthing(stem: &str) -> Option<(Client, &str)> {
    let i = stem.rfind(".sync-conflict-")?;
    let mut parts = stem[i + ".sync-conflict-".len()..].split('-');
    let (date, time, device) = (parts.next()?, parts.next()?, parts.next());
    let ok = digits(date, 8) && digits(time, 6) && device.is_none_or(|d| d.len() == 7 && d.bytes().all(|b| b.is_ascii_alphanumeric())) && parts.next().is_none();
    ok.then_some((Client::Syncthing, &stem[..i]))
}

/// `Name_conflict-YYYYMMDD-HHMMSS` (ownCloud); a counter `-N` may follow.
fn owncloud(stem: &str) -> Option<(Client, &str)> {
    let i = stem.rfind("_conflict-")?;
    let mut parts = stem[i + "_conflict-".len()..].split('-');
    let (date, time, counter) = (parts.next()?, parts.next()?, parts.next());
    let ok = digits(date, 8) && digits(time, 6) && counter.is_none_or(number) && parts.next().is_none();
    ok.then_some((Client::OwnCloud, &stem[..i]))
}

fn date(s: &str) -> bool {
    let b = s.as_bytes();
    s.len() == 10 && digits(&s[..4], 4) && b[4] == b'-' && digits(&s[5..7], 2) && b[7] == b'-' && digits(&s[8..], 2)
}

/// `Name (inner)` -> (`Name`, `inner`): the group at the end of the stem.
fn split_group(stem: &str) -> Option<(&str, &str)> {
    let body = stem.strip_suffix(')')?;
    let open = body.rfind(" (")?;
    let inner = &body[open + 2..];
    (!inner.contains(['(', ')'])).then_some((&body[..open], inner))
}

/// The patterns that end in a group ` (…)`, and Dropbox's counter ` (N)` after it.
fn parenthesised(stem: &str) -> Option<(Client, &str)> {
    if let Some((base, inner)) = split_group(stem) {
        if let Some(client) = classify(inner) {
            return Some((client, base));
        }
        if number(inner) {
            let (base, inner) = split_group(base)?;
            return (classify(inner) == Some(Client::Dropbox)).then_some((Client::Dropbox, base));
        }
    }
    None
}

fn classify(inner: &str) -> Option<Client> {
    if let Some(rest) = inner.strip_prefix("conflict ") {
        // `YYYY-MM-DD HHMM`, and ` N` when the name was taken
        let mut p = rest.split(' ');
        let (day, time, counter) = (p.next()?, p.next()?, p.next());
        return (date(day) && digits(time, 4) && counter.is_none_or(number) && p.next().is_none()).then_some(Client::Shale);
    }
    if let Some(rest) = inner.strip_prefix("SFConflict ") {
        // `<user> YYYY-MM-DD-HH-MM-SS`
        let (user, stamp) = rest.rsplit_once(' ')?;
        let parts: Vec<&str> = stamp.split('-').collect();
        let ok = !user.is_empty() && parts.len() == 6 && digits(parts[0], 4) && parts[1..].iter().all(|p| digits(p, 2));
        return ok.then_some(Client::Seafile);
    }
    // Dropbox: `<User>'s conflicted copy YYYY-MM-DD`; Nextcloud: `conflicted copy YYYY-MM-DD HHMMSS`
    let at = inner.find("conflicted copy ")?;
    let (who, after) = (&inner[..at], &inner[at + "conflicted copy ".len()..]);
    if !who.is_empty() && !who.ends_with(' ') {
        return None;
    }
    let mut p = after.split(' ');
    let (day, time) = (p.next()?, p.next());
    if !date(day) || p.next().is_some() {
        return None;
    }
    match time {
        None => Some(Client::Dropbox),
        Some(t) if who.is_empty() && digits(t, 6) => Some(Client::Nextcloud),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn of(n: &str) -> Option<(Client, String)> {
        original_of(n)
    }

    // spec: NET-015
    #[test]
    fn net_015_the_patterns_of_every_client_give_their_original() {
        let want = |n: &str, c: Client, o: &str| assert_eq!(of(n), Some((c, o.to_string())), "{n}");
        want("N.sync-conflict-20261121-142440-7JJWLPP.md", Client::Syncthing, "N.md");
        want("N.sync-conflict-20261121-142440.md", Client::Syncthing, "N.md");
        want("Plan.excalidraw.sync-conflict-20261121-142440-7JJWLPP.md", Client::Syncthing, "Plan.excalidraw.md");
        want("Plan.sync-conflict-20261121-142440-7JJWLPP.excalidraw.md", Client::Syncthing, "Plan.excalidraw.md");
        want("My note (Raba's conflicted copy 2026-11-21).md", Client::Dropbox, "My note.md");
        want("My note (Raba's conflicted copy 2026-11-21) (1).md", Client::Dropbox, "My note.md");
        want("My note (conflicted copy 2026-11-21 142440).md", Client::Nextcloud, "My note.md");
        want("My note_conflict-20261121-142440.md", Client::OwnCloud, "My note.md");
        want("My note_conflict-20261121-142440-2.md", Client::OwnCloud, "My note.md");
        want("My note (SFConflict raba 2026-11-21-14-24-40).md", Client::Seafile, "My note.md");
        want("My note (conflict 2026-11-21 1424).md", Client::Shale, "My note.md");
        want("My note (conflict 2026-11-21 1424 2).md", Client::Shale, "My note.md");
        want("Plan (conflict 2026-11-21 1424).excalidraw.md", Client::Shale, "Plan.excalidraw.md");
        want("Folder (1) (conflict 2026-11-21 1424).MD", Client::Shale, "Folder (1).MD");
    }

    // spec: NET-015
    #[test]
    fn net_015_names_that_only_look_alike_are_not_conflict_copies() {
        for n in [
            "Plan 2.md",
            "Plan (1).md",
            "Plan-LAPTOP.md",
            "Plan (conflict).md",
            "Plan (conflict 2026-11-21).md",
            "Plan (conflict 26-11-21 1424).md",
            "Plan (conflict 2026-11-21 1424 x).md",
            "Plan (conflicted copy).md",
            "Plan (conflicted copy 2026-11-21 14).md",
            "Plan (conflicted copy 2026-11-21) (x).md",
            "Plan (Raba's conflicted copy 2026-11-21 142440).md",
            "Plan.sync-conflict-2026-1.md",
            "Plan.sync-conflict-20261121-142440-AB.md",
            "Plan.sync-conflict-20261121-142440-7JJWLPP-9.md",
            "Plan_conflict-1.md",
            "Plan_conflict-20261121-142440-x.md",
            "Plan (SFConflict 2026-11-21-14-24-40).md",
            "Plan (SFConflict raba 2026-11-21).md",
            ".sync-conflict-20261121-142440-7JJWLPP.md",
            " (conflict 2026-11-21 1424).md",
            "Plan (conflict 2026-11-21 1424) (1).md",
            "Plan.sync-conflict-20261121-142440-7JJWLPP.txt",
            "Plan (conflict 2026-11-21 1424).png",
            "Plan",
        ] {
            assert_eq!(of(n), None, "{n}");
        }
    }

    // spec: NET-015
    #[test]
    fn net_015_every_client_has_a_label() {
        let all = [Client::Syncthing, Client::Dropbox, Client::Nextcloud, Client::OwnCloud, Client::Seafile, Client::Shale];
        let labels: std::collections::HashSet<_> = all.iter().map(|c| c.label()).collect();
        assert_eq!(labels.len(), all.len());
    }
}
