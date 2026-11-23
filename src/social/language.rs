//! The language of a post (SOC-065): the codes the picker offers (ISO 639-1), and the default,
//! which is the language of the system's locale. Mastodon takes it as `language`, Bluesky as
//! `langs` of the record.

/// The languages of the picker: ISO 639-1 code and the name in English.
pub const LANGUAGES: [(&str, &str); 40] = [
    ("ar", "Arabic"),
    ("bg", "Bulgarian"),
    ("ca", "Catalan"),
    ("cs", "Czech"),
    ("cy", "Welsh"),
    ("da", "Danish"),
    ("de", "German"),
    ("el", "Greek"),
    ("en", "English"),
    ("eo", "Esperanto"),
    ("es", "Spanish"),
    ("et", "Estonian"),
    ("eu", "Basque"),
    ("fa", "Persian"),
    ("fi", "Finnish"),
    ("fr", "French"),
    ("ga", "Irish"),
    ("gl", "Galician"),
    ("he", "Hebrew"),
    ("hi", "Hindi"),
    ("hr", "Croatian"),
    ("hu", "Hungarian"),
    ("id", "Indonesian"),
    ("is", "Icelandic"),
    ("it", "Italian"),
    ("ja", "Japanese"),
    ("ko", "Korean"),
    ("lt", "Lithuanian"),
    ("lv", "Latvian"),
    ("nb", "Norwegian"),
    ("nl", "Dutch"),
    ("pl", "Polish"),
    ("pt", "Portuguese"),
    ("ro", "Romanian"),
    ("ru", "Russian"),
    ("sk", "Slovak"),
    ("sv", "Swedish"),
    ("tr", "Turkish"),
    ("uk", "Ukrainian"),
    ("zh", "Chinese"),
];

/// The name of a code ("de" → "German"); the code itself for one the list does not know; "No language" for "".
pub fn name(code: &str) -> String {
    if code.is_empty() {
        return "No language".into();
    }
    LANGUAGES.iter().find(|(c, _)| *c == code).map_or_else(|| code.to_string(), |(_, n)| format!("{n} ({code})"))
}

/// A code the post may carry: two lowercase letters ("" is no language).
pub fn is_code(code: &str) -> bool {
    code.is_empty() || (code.len() == 2 && code.bytes().all(|b| b.is_ascii_lowercase()))
}

/// The language of a locale name such as "de_DE.UTF-8" or "en-US": its first two letters when
/// the list knows them.
pub fn from_locale(locale: &str) -> Option<String> {
    let code: String = locale.chars().take_while(|c| c.is_ascii_alphabetic()).collect::<String>().to_ascii_lowercase();
    LANGUAGES.iter().any(|(c, _)| *c == code).then_some(code)
}

/// The language of the system's locale (`LC_ALL`, `LC_MESSAGES`, `LANG`), "en" without a usable one.
pub fn system() -> String {
    ["LC_ALL", "LC_MESSAGES", "LANG"].iter().filter_map(|k| std::env::var(k).ok()).find_map(|v| from_locale(&v)).unwrap_or_else(|| "en".into())
}
