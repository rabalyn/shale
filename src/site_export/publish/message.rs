//! The message of a publish commit (SITE-072, SITE-073): a subject and the trailers that name the
//! device, the time and the vault, like the trailers of SYNC-036. They are what the next publish
//! reads from the remote (SITE-074) and what the settings show (SITE-072).

use crate::calendar::time;

pub const DEVICE: &str = "Shale-Device";
pub const TIME: &str = "Shale-Time";
pub const VAULT: &str = "Shale-Vault";
pub const VAULT_NAME: &str = "Shale-Vault-Name";

/// What a publish commit says about who made it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Meta {
    pub device: Option<String>,
    /// Seconds since 1970 (UTC).
    pub time: Option<i64>,
    pub vault_id: Option<String>,
    pub vault_name: Option<String>,
}

fn clean(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).collect::<String>().trim().to_string()
}

/// `2026-11-24T14:05:09Z` for a time in seconds since 1970.
pub fn iso(secs: i64) -> String {
    let (days, sod) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let (y, m, d) = time::civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", sod / 3600, sod % 3600 / 60, sod % 60)
}

/// The seconds of an [`iso`] text.
pub fn parse_iso(text: &str) -> Option<i64> {
    let (date, clock) = text.trim().strip_suffix('Z')?.split_once('T')?;
    let days = time::parse_ymd(date)?;
    let mut parts = clock.split(':').map(|p| p.parse::<i64>().ok());
    let (h, m, s) = (parts.next()??, parts.next()??, parts.next()??);
    (parts.next().is_none() && (0..24).contains(&h) && (0..60).contains(&m) && (0..61).contains(&s)).then(|| days * 86_400 + h * 3600 + m * 60 + s)
}

/// The message of the snapshot commit: a subject, then the trailers (SITE-073).
pub fn compose(notes: usize, meta: &Meta) -> String {
    let name = clean(meta.vault_name.as_deref().unwrap_or(""));
    let mut out = format!("Publish {}: {notes} {}\n\n", if name.is_empty() { "vault" } else { &name }, if notes == 1 { "note" } else { "notes" });
    let mut line = |key: &str, value: Option<String>| {
        if let Some(v) = value.filter(|v| !v.is_empty()) {
            out.push_str(&format!("{key}: {v}\n"));
        }
    };
    line(DEVICE, meta.device.as_deref().map(clean));
    line(TIME, meta.time.map(iso));
    line(VAULT, meta.vault_id.as_deref().map(clean));
    line(VAULT_NAME, Some(name));
    out
}

/// Read the trailers of a commit message; a message without them (made by another tool) gives nothing.
pub fn parse(message: &str) -> Meta {
    let mut meta = Meta::default();
    for line in message.lines().skip(1) {
        let Some((key, value)) = line.split_once(':') else { continue };
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        match key.trim() {
            DEVICE => meta.device = Some(value.to_string()),
            TIME => meta.time = parse_iso(value),
            VAULT => meta.vault_id = Some(value.to_ascii_lowercase()),
            VAULT_NAME => meta.vault_name = Some(value.to_string()),
            _ => {}
        }
    }
    meta
}

/// A time as the person reads it: local, `2026-11-24 15:05`.
pub fn local(secs: i64) -> String {
    let (days, sod) = time::utc_to_local(secs);
    let (y, m, d) = time::civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02} {}", time::hhmm(sod))
}
