//! The threads of imported mails (IMP-067…IMP-073, ADR-0032): the properties, the matching by reply
//! headers, the chain of `previous` / `next`, and the overview's data. Real notes in a temporary
//! vault, imported through `import::import` as the dialog does.

use crate::import::{self, Imported, Item, Options};
use crate::thread::{self, Ident, Index, Meta};
use crate::vault::Vault;
use std::path::{Path, PathBuf};

fn vault(tag: &str) -> Vault {
    let d = std::env::temp_dir().join(format!("shale-thread-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    Vault::open(&d).unwrap()
}

/// A plain mail. `refs`: the ids of its `References` header (without brackets).
pub(crate) fn eml(id: &str, date: &str, subject: &str, reply: Option<&str>, refs: &[&str]) -> Vec<u8> {
    let mut s = format!("Message-ID: <{id}>\r\nDate: {date}\r\nFrom: Ann <ann@x.example>\r\nTo: Bob <bob@x.example>\r\nSubject: {subject}\r\n");
    if let Some(r) = reply {
        s.push_str(&format!("In-Reply-To: <{r}>\r\n"));
    }
    if !refs.is_empty() {
        s.push_str(&format!("References: {}\r\n", refs.iter().map(|r| format!("<{r}>")).collect::<Vec<_>>().join(" ")));
    }
    s.push_str(&format!("MIME-Version: 1.0\r\nContent-Type: text/plain; charset=UTF-8\r\n\r\nText of {id}\r\n"));
    s.into_bytes()
}

pub(crate) const D16: &str = "Mon, 16 Nov 2026 12:00:00 +0000";
pub(crate) const D17: &str = "Tue, 17 Nov 2026 12:00:00 +0000";
pub(crate) const D18: &str = "Wed, 18 Nov 2026 12:00:00 +0000";
pub(crate) const D19: &str = "Thu, 19 Nov 2026 12:00:00 +0000";

fn options(folder: &str) -> Options {
    Options { folder: PathBuf::from(folder), tags: vec![], keep_original: false, keep_docx: false, people_dir: "Adressbuch".into(), header_list: false, cut: Default::default() }
}

fn items(v: &Vault, raw: Vec<u8>) -> Vec<Item> {
    let mut items = vec![import::eml_item(raw, None)];
    import::find_continues(v, &mut items);
    items
}

/// Import one mail the way the dialog does (the suggestion as found), `tweak` is the person's choice.
fn import_with(v: &mut Vault, raw: Vec<u8>, folder: &str, tweak: impl FnOnce(&mut Item)) -> (PathBuf, Imported) {
    let mut items = items(v, raw);
    tweak(&mut items[0]);
    let done = import::import(v, &items, &options(folder));
    assert!(done.errors.is_empty(), "{:?}", done.errors);
    (done.notes[0].clone(), done)
}

fn import_mail(v: &mut Vault, raw: Vec<u8>, folder: &str) -> (PathBuf, Imported) {
    import_with(v, raw, folder, |_| {})
}

fn text(v: &Vault, rel: &Path) -> String {
    std::fs::read_to_string(v.abs(rel)).unwrap()
}

/// The value of a front matter line.
fn prop(text: &str, key: &str) -> Option<String> {
    text.lines().skip(1).take_while(|l| *l != "---").find_map(|l| l.strip_prefix(&format!("{key}: ")).map(String::from))
}

// spec: IMP-067
#[test]
fn imp_067_a_mail_note_has_the_ids_of_its_thread_in_one_header() {
    let mut v = vault("ids");
    // a mail that starts a thread: its own id is its thread, nothing else
    let (root, _) = import_mail(&mut v, eml("a@h", D16, "Plan", None, &[]), "Mail");
    let t = text(&v, &root);
    assert_eq!(prop(&t, "message-id").as_deref(), Some("\"<a@h>\""));
    assert_eq!(prop(&t, "thread-id").as_deref(), Some("\"<a@h>\""), "{t}");
    assert!(prop(&t, "in-reply-to").is_none() && !t.contains("references:") && prop(&t, "previous").is_none() && prop(&t, "next").is_none(), "{t}");
    // a reply: the ids as the headers had them, the thread is the first of References
    let raw = eml("c@h", D18, "Re: Plan", Some("b@h"), &["a@h", "b@h"]);
    let (reply, _) = import_with(&mut v, raw, "Elsewhere", |it| it.continues = None);
    let t = text(&v, &reply);
    assert_eq!(prop(&t, "in-reply-to").as_deref(), Some("\"<b@h>\""), "{t}");
    assert_eq!(prop(&t, "thread-id").as_deref(), Some("\"<a@h>\""));
    assert!(t.contains("references:\n  - \"<a@h>\"\n  - \"<b@h>\"\n"), "{t}");
    assert_eq!(t.matches("\n---\n").count(), 1, "the header is written once: {t}");
    assert!(prop(&t, "previous").is_none(), "no match was ticked: no chain");
    // only In-Reply-To: that is the thread
    let (only, _) = import_with(&mut v, eml("e@h", D19, "Re", Some("d@h"), &[]), "Elsewhere", |it| it.continues = None);
    assert_eq!(prop(&text(&v, &only), "thread-id").as_deref(), Some("\"<d@h>\""));
}

// spec: IMP-068
#[test]
fn imp_068_a_reply_is_found_by_its_in_reply_to_and_by_references_alone() {
    let mut v = vault("match");
    let (a, _) = import_mail(&mut v, eml("a@h", D16, "Plan", None, &[]), "Mail");
    // In-Reply-To names the note
    let found = items(&v, eml("b@h", D17, "Something else entirely", Some("a@h"), &[]));
    let c = found[0].continues.as_ref().expect("continues the note");
    assert_eq!((c.candidates.iter().map(|c| &c.rel).collect::<Vec<_>>(), c.on), (vec![&a], true), "ticked by default");
    // only References name the note (a mail client that sends no In-Reply-To)
    let found = items(&v, eml("c@h", D17, "No subject like it", None, &["x@h", "a@h"]));
    assert_eq!(found[0].continues.as_ref().unwrap().candidates[0].rel, a);
    // angle brackets and the case of the header do not matter: ids compare bare
    assert_eq!(thread::bare_id(" <a@h> "), "a@h");
}

// spec: IMP-068
#[test]
fn imp_068_no_match_without_a_shared_id_and_never_by_subject_sender_or_text() {
    let mut v = vault("nomatch");
    import_mail(&mut v, eml("a@h", D16, "Plan", None, &[]), "Mail");
    // the same subject (with Re:), the same sender, the same text: but no id of the note
    let found = items(&v, eml("z@h", D17, "Re: Plan", None, &[]));
    assert!(found[0].continues.is_none(), "no reply header, no suggestion");
    let found = items(&v, eml("z@h", D17, "Plan", Some("other@h"), &["third@h"]));
    assert!(found[0].continues.is_none());
    // an empty vault finds nothing either
    assert!(items(&vault("empty"), eml("b@h", D17, "Re: Plan", Some("a@h"), &[]))[0].continues.is_none());
}

// spec: IMP-068
#[test]
fn imp_068_several_matches_are_offered_the_note_it_replies_to_first() {
    let mut v = vault("several");
    let (a, _) = import_mail(&mut v, eml("a@h", D16, "Plan", None, &[]), "Mail");
    let (b, _) = import_mail(&mut v, eml("b@h", D17, "Re: Plan", Some("a@h"), &["a@h"]), "Mail");
    let (c, _) = import_mail(&mut v, eml("c@h", D18, "Re: Re: Plan", Some("b@h"), &["a@h", "b@h"]), "Mail");
    // d replies to b, and a and c are in its thread too: b first (In-Reply-To), then the others of References, then the thread
    let found = items(&v, eml("d@h", D19, "Re", Some("b@h"), &["a@h", "b@h"]));
    let order: Vec<&PathBuf> = found[0].continues.as_ref().unwrap().candidates.iter().map(|c| &c.rel).collect();
    assert_eq!(order, vec![&b, &a, &c], "reply target, then References, then the same thread newest first");
    // an older mail than a note names that note: it is matched the other way round
    let mut vv = vault("older");
    let (late, _) = import_with(&mut vv, eml("b@h", D17, "Re", Some("a@h"), &["a@h"]), "Mail", |_| {});
    let found = items(&vv, eml("a@h", D16, "Plan", None, &[]));
    assert_eq!(found[0].continues.as_ref().unwrap().candidates[0].rel, late, "the note that names the mail is found");
}

// spec: IMP-070
#[test]
fn imp_070_a_newer_mail_is_appended_the_last_note_gets_its_next_and_nothing_else_changes() {
    let mut v = vault("append");
    let (a, _) = import_mail(&mut v, eml("a@h", D16, "Plan", None, &[]), "Mail");
    let a_before = text(&v, &a);
    let (b, done) = import_mail(&mut v, eml("b@h", D17, "Re: Plan", Some("a@h"), &["a@h"]), "Mail");
    let name = |p: &Path| p.file_stem().unwrap().to_string_lossy().into_owned();
    assert_eq!(prop(&text(&v, &b), "previous"), Some(format!("\"[[{}]]\"", name(&a))));
    assert!(prop(&text(&v, &b), "next").is_none());
    assert_eq!(done.touched, vec![a.clone()], "one other note is rewritten");
    // only the next line was added to the old note: front matter lines before are identical
    let a_after = text(&v, &a);
    assert_eq!(a_after.replace(&format!("next: \"[[{}]]\"\n", name(&b)), ""), a_before);
    let (c, done) = import_mail(&mut v, eml("c@h", D18, "Re: Re: Plan", Some("b@h"), &["a@h", "b@h"]), "Mail");
    assert_eq!(done.touched, vec![b.clone()], "the first note is not touched again");
    assert_eq!(prop(&text(&v, &b), "next"), Some(format!("\"[[{}]]\"", name(&c))));
    assert_eq!(text(&v, &a), a_after);
}

// spec: IMP-068, IMP-070
#[test]
fn imp_070_an_unticked_continuation_writes_the_ids_only_and_touches_no_other_note() {
    let mut v = vault("unticked");
    let (a, _) = import_mail(&mut v, eml("a@h", D16, "Plan", None, &[]), "Mail");
    let a_before = text(&v, &a);
    let (b, done) = import_with(&mut v, eml("b@h", D17, "Re: Plan", Some("a@h"), &["a@h"]), "Mail", |it| it.continues.as_mut().unwrap().on = false);
    let t = text(&v, &b);
    assert!(prop(&t, "previous").is_none() && prop(&t, "next").is_none());
    assert_eq!(prop(&t, "thread-id").as_deref(), Some("\"<a@h>\""), "it still has its ids");
    assert!(done.touched.is_empty());
    assert_eq!(text(&v, &a), a_before);
}

// spec: IMP-070
#[test]
fn imp_070_the_order_is_the_date_and_a_tie_breaks_by_the_id_text() {
    let mut v = vault("ties");
    let (a, _) = import_mail(&mut v, eml("a@h", D16, "Plan", None, &[]), "Mail");
    // two replies of the same minute: the id text decides, the same on every device
    let (z, _) = import_mail(&mut v, eml("z@h", D17, "z", Some("a@h"), &["a@h"]), "Mail");
    let found = items(&v, eml("m@h", D17, "m", Some("a@h"), &["a@h"]));
    let plan = found[0].continues.as_ref().unwrap().plan().unwrap();
    assert_eq!((plan.previous.as_ref(), plan.next.as_ref()), (Some(&a), Some(&z)), "m@h sorts before z@h");
    let found = items(&v, eml("zz@h", D17, "zz", Some("a@h"), &["a@h"]));
    let c = found[0].continues.as_ref().unwrap();
    assert_eq!(c.candidates.len(), 2, "both notes of the thread are candidates");
    assert!(c.plans.iter().all(|p| !p.is_older() && p.previous.as_ref() == Some(&z)), "zz@h comes after z@h: appended");
}

// spec: IMP-071
#[test]
fn imp_071_an_older_mail_is_inserted_with_update_and_changes_only_its_two_neighbours() {
    let mut v = vault("update");
    let (a, _) = import_mail(&mut v, eml("a@h", D16, "Plan", None, &[]), "Mail");
    let (c, _) = import_mail(&mut v, eml("c@h", D18, "Late", Some("a@h"), &["a@h"]), "Mail");
    let (d, _) = import_mail(&mut v, eml("d@h", D19, "Later", Some("c@h"), &["a@h", "c@h"]), "Mail");
    let before: Vec<String> = [&a, &c, &d].iter().map(|p| text(&v, p)).collect();
    // b lies between a and c
    let found = items(&v, eml("b@h", D17, "Middle", Some("a@h"), &["a@h"]));
    let c0 = found[0].continues.as_ref().unwrap();
    let plan = c0.plan().unwrap();
    assert!(plan.is_older() && import::older_questions(&found)[&c0.group].1 == 2, "the dialog asks and names 2 notes: {plan:?}");
    assert!(c0.update, "Update is the start answer");
    let (b, done) = import_mail(&mut v, eml("b@h", D17, "Middle", Some("a@h"), &["a@h"]), "Mail");
    let name = |p: &Path| p.file_stem().unwrap().to_string_lossy().into_owned();
    let tb = text(&v, &b);
    assert_eq!((prop(&tb, "previous"), prop(&tb, "next")), (Some(format!("\"[[{}]]\"", name(&a))), Some(format!("\"[[{}]]\"", name(&c)))));
    assert_eq!(done.touched.len(), 2, "never more than the two neighbours");
    assert!(done.touched.contains(&a) && done.touched.contains(&c));
    assert_eq!(prop(&text(&v, &a), "next"), Some(format!("\"[[{}]]\"", name(&b))));
    assert_eq!(prop(&text(&v, &c), "previous"), Some(format!("\"[[{}]]\"", name(&b))));
    assert_eq!(text(&v, &d), before[2], "the note after is not touched");
    // c and a changed only in that one line
    let line = |t: &str, k: &str| t.lines().find(|l| l.starts_with(k)).map(String::from);
    let only_changed = |old: &str, new: &str, key: &str| {
        let strip = |t: &str| t.lines().filter(|l| !l.starts_with(&format!("{key}:"))).collect::<Vec<_>>().join("\n");
        strip(old) == strip(new) && line(old, key) != line(new, key)
    };
    assert!(only_changed(&before[0], &text(&v, &a), "next"), "a: only its next line");
    assert!(only_changed(&before[1], &text(&v, &c), "previous"), "c: only its previous line");
}

// spec: IMP-071
#[test]
fn imp_071_leave_the_chain_writes_the_older_mail_without_links_and_touches_nothing() {
    let mut v = vault("leave");
    let (a, _) = import_mail(&mut v, eml("a@h", D16, "Plan", None, &[]), "Mail");
    let (c, _) = import_mail(&mut v, eml("c@h", D18, "Late", Some("a@h"), &["a@h"]), "Mail");
    let (ta, tc) = (text(&v, &a), text(&v, &c));
    let (b, done) = import_with(&mut v, eml("b@h", D17, "Middle", Some("a@h"), &["a@h"]), "Mail", |it| it.continues.as_mut().unwrap().update = false);
    let tb = text(&v, &b);
    assert!(prop(&tb, "previous").is_none() && prop(&tb, "next").is_none(), "{tb}");
    assert_eq!(prop(&tb, "thread-id").as_deref(), Some("\"<a@h>\""));
    assert!(done.touched.is_empty());
    assert_eq!((text(&v, &a), text(&v, &c)), (ta, tc));
    // the overview does not depend on the chain: the thread lists b between a and c
    let rows = Index::build(&v).thread_of(&b).unwrap();
    assert_eq!(rows.iter().map(|r| &r.rel).collect::<Vec<_>>(), vec![&a, &b, &c]);
}

// spec: IMP-071
#[test]
fn imp_071_the_oldest_mail_of_all_changes_one_neighbour() {
    let mut v = vault("oldest");
    let (b, _) = import_mail(&mut v, eml("b@h", D17, "Re", Some("a@h"), &["a@h"]), "Mail");
    let found = items(&v, eml("a@h", D16, "Plan", None, &[]));
    let c = found[0].continues.as_ref().unwrap();
    assert_eq!(c.candidates[0].rel, b, "it is found because the note names it");
    let plan = c.plan().unwrap();
    assert!(plan.previous.is_none() && plan.next.as_ref() == Some(&b) && import::older_questions(&found)[&c.group].1 == 1);
    let (a, done) = import_mail(&mut v, eml("a@h", D16, "Plan", None, &[]), "Mail");
    assert_eq!(done.touched, vec![b.clone()]);
    assert!(prop(&text(&v, &a), "previous").is_none() && prop(&text(&v, &a), "next").is_some());
    assert!(prop(&text(&v, &b), "previous").is_some());
}

// spec: IMP-073
#[test]
fn imp_073_a_rename_of_a_note_keeps_the_chain_because_previous_and_next_are_links() {
    let mut v = vault("rename");
    let (a, _) = import_mail(&mut v, eml("a@h", D16, "Plan", None, &[]), "Mail");
    let (b, _) = import_mail(&mut v, eml("b@h", D17, "Re: Plan", Some("a@h"), &["a@h"]), "Mail");
    let (c, _) = import_mail(&mut v, eml("c@h", D18, "Re", Some("b@h"), &["a@h", "b@h"]), "Mail");
    // rename the middle note: its neighbours' links follow, and so does the overview
    let new = PathBuf::from("Mail/Renamed plan.md");
    v.rename(&b, &new, true).unwrap();
    assert!(prop(&text(&v, &a), "next").unwrap().contains("[[Renamed plan]]"), "{}", text(&v, &a));
    assert!(prop(&text(&v, &c), "previous").unwrap().contains("[[Renamed plan]]"), "{}", text(&v, &c));
    // and a move into another folder
    let (moved, _) = v.move_into(&new, Path::new("")).unwrap();
    assert_eq!(moved, PathBuf::from("Renamed plan.md"));
    assert!(prop(&text(&v, &a), "next").unwrap().contains("Renamed plan"));
    let rows = Index::build(&v).thread_of(&a).unwrap();
    assert_eq!(rows.iter().map(|r| &r.rel).collect::<Vec<_>>(), vec![&a, &moved, &c], "the thread ids do not change");
}

// spec: IMP-073
#[test]
fn imp_073_an_old_note_with_only_a_message_id_is_still_the_parent_of_a_new_mail() {
    let mut v = vault("old");
    v.create_file(Path::new("Mail/2026-11-16 Old plan.md"), "---\ntype: email\nsubject: \"Old plan\"\ndate: 2026-11-16 12:00\nmessage-id: \"<a@h>\"\n---\n\n# Old plan\n").unwrap();
    let old = PathBuf::from("Mail/2026-11-16 Old plan.md");
    let before = text(&v, &old);
    let (b, done) = import_mail(&mut v, eml("b@h", D17, "Re: Old plan", Some("a@h"), &["a@h"]), "Other");
    assert_eq!(done.touched, vec![old.clone()]);
    // the old note gained its next line and nothing else; no command rewrote anything more
    let name = b.file_stem().unwrap().to_string_lossy().into_owned();
    assert_eq!(text(&v, &old).replace(&format!("next: \"[[{name}]]\"\n"), ""), before, "{}", text(&v, &old));
    assert!(prop(&text(&v, &b), "previous").unwrap().contains("2026-11-16 Old plan"));
}

// spec: NAV-070
#[test]
fn nav_070_the_thread_of_a_note_is_its_notes_with_the_same_thread_id_oldest_first() {
    let mut v = vault("overview");
    let (a, _) = import_mail(&mut v, eml("a@h", D16, "Plan", None, &[]), "Mail");
    let (c, _) = import_mail(&mut v, eml("c@h", D18, "Third", Some("a@h"), &["a@h"]), "Mail");
    let (b, _) = import_with(&mut v, eml("b@h", D17, "Second", Some("a@h"), &["a@h"]), "Other folder", |it| it.continues.as_mut().unwrap().update = false);
    // another thread, and a note without any
    let (x, _) = import_mail(&mut v, eml("x@h", D17, "Other", None, &[]), "Mail");
    v.create_file(Path::new("Plain.md"), "# Plain\n").unwrap();
    let idx = Index::build(&v);
    let rows = idx.thread_of(&c).unwrap();
    assert_eq!(rows.iter().map(|r| &r.rel).collect::<Vec<_>>(), vec![&a, &b, &c], "oldest first, wherever the notes are");
    assert_eq!(rows[0].who, "Ann <ann@x.example>");
    assert_eq!(rows[0].date_key.len(), 16);
    assert_eq!(idx.thread_of(&x).unwrap().len(), 1);
    assert!(idx.thread_of(Path::new("Plain.md")).is_none(), "no thread-id: no thread");
    // the stepping order is the overview's
    assert_eq!(thread::step(&rows, &b, 1), Some(c.clone()));
    assert_eq!(thread::step(&rows, &b, -1), Some(a.clone()));
    assert_eq!(thread::step(&rows, &a, -1), None, "nothing before the first");
    assert_eq!(thread::step(&rows, &c, 1), None, "nothing after the last");
    assert_eq!(thread::step(&rows, Path::new("Plain.md"), 1), None);
}

// spec: IMP-067
#[test]
fn imp_067_the_thread_data_is_read_from_the_front_matter_with_ids_compared_bare() {
    let text = "---\ntype: email\nmessage-id: \"<a@h>\"\nin-reply-to: \"<b@h>\"\nreferences:\n  - \"<c@h>\"\n  - d@h\nthread-id: \"<c@h>\"\ndate: 2026-11-17 09:05\nfrom: \"Ann <ann@x>\"\n---\nbody: x\n";
    let m = Meta::parse(text).unwrap();
    assert_eq!((m.ids.as_slice(), m.in_reply_to.as_str(), m.thread_id.as_str(), m.date_key.as_str(), m.who.as_str()), (&["a@h".to_string()][..], "b@h", "c@h", "2026-11-17 09:05", "Ann <ann@x>"));
    assert_eq!(m.references, ["c@h", "d@h"]);
    assert!(Meta::parse("# no front matter\nmessage-id: <x>").is_none());
    assert!(Meta::parse("---\ntitle: x\n---\n").is_none());
    // a saved self-thread of one author matches by every id of thread-ids
    let post = "---\ntype: social-post\nnetwork: mastodon\npost-id: \"1\"\nthread-ids: [\"1\", \"2\"]\nauthor: \"@a\"\n---\n";
    let m = Meta::parse(post).unwrap();
    assert_eq!((m.ids.clone(), m.network.as_str(), m.who.as_str()), (vec!["1".to_string(), "2".to_string()], "mastodon", "@a"));
    assert_eq!(thread::date_key("2026-11-20"), "2026-11-20 00:00");
    assert_eq!(thread::date_key("2026-11-20T14:30:05+01:00"), "2026-11-20 14:30");
    assert_eq!(thread::date_key("Thu, 13 Nov 2026"), "");
}

// spec: IMP-070
#[test]
fn imp_070_a_property_line_is_replaced_in_place_or_added_and_nothing_else_changes() {
    let t = "---\na: 1\nnext: \"[[Old]]\"\nb: 2\n---\n\n# Body\nnext: not a property\n";
    assert_eq!(thread::set_property(t, "next", "\"[[New]]\"").unwrap(), "---\na: 1\nnext: \"[[New]]\"\nb: 2\n---\n\n# Body\nnext: not a property\n");
    assert_eq!(thread::set_property(t, "previous", "\"[[P]]\"").unwrap(), "---\na: 1\nnext: \"[[Old]]\"\nb: 2\nprevious: \"[[P]]\"\n---\n\n# Body\nnext: not a property\n");
    assert!(thread::set_property(t, "next", "\"[[Old]]\"").is_none(), "the same value: no change");
    assert!(thread::set_property("# no header\n", "next", "x").is_none());
    // the line ends of the file are kept, a block value is replaced as a whole
    let crlf = "---\r\nrefs:\r\n  - x\r\nb: 2\r\n---\r\ntext\r\n";
    assert_eq!(thread::set_property(crlf, "refs", "y").unwrap(), "---\r\nrefs: y\r\nb: 2\r\n---\r\ntext\r\n");
    assert_eq!(thread::set_property(crlf, "n", "1").unwrap(), "---\r\nrefs:\r\n  - x\r\nb: 2\r\nn: 1\r\n---\r\ntext\r\n");
}

// spec: IMP-068
#[test]
fn imp_068_a_post_is_matched_only_in_its_own_network_and_the_ident_names_the_root() {
    let mut v = vault("network");
    v.create_file(Path::new("P1.md"), "---\ntype: social-post\nnetwork: mastodon\npost-id: \"1\"\ndate: 2026-11-16 10:00\n---\n").unwrap();
    let idx = Index::build(&v);
    let reply = |network: &str| Ident { id: "2".into(), in_reply_to: "1".into(), references: vec![], network: network.into(), date_key: "2026-11-16 11:00".into() };
    assert_eq!(idx.matches(&reply("mastodon")).len(), 1);
    assert!(idx.matches(&reply("bluesky")).is_empty(), "post ids are only unique per network");
    assert_eq!(reply("mastodon").thread_id(), "1");
    assert_eq!(Ident { id: "9".into(), ..Default::default() }.thread_id(), "9", "a post that starts a thread is its own root");
}

// ---- posts (SOC-070…SOC-072) ----

use crate::social::note::SavedPost;
use crate::social::{tests::post, Network};

/// A saved post of the day `day` of November 2026 at noon UTC. `reply`: the post it replies to,
/// `root`: the root of the conversation as the network says it.
fn saved_post(id: &str, day: i64, reply: Option<&str>, root: Option<&str>) -> SavedPost {
    let p = post(id, "ada", 1_793_620_800 + (day - 1) * 86_400, reply);
    SavedPost { thread: vec![p], reposted_by: None, media: vec![], failed_media: vec![], thread_error: None, replies: vec![], root: root.map(String::from) }
}

fn post_items(posts: Vec<SavedPost>) -> Vec<Item> {
    posts.into_iter().map(|s| import::social_item(s, "Mastodon", false)).collect()
}

/// Save one post the way the dialog does; `tweak` is the person's choice.
fn save_post(v: &mut Vault, saved: SavedPost, folder: &str, tweak: impl FnOnce(&mut Item)) -> (PathBuf, Imported) {
    let mut items = post_items(vec![saved]);
    import::find_continues(v, &mut items);
    tweak(&mut items[0]);
    let done = import::import(v, &items, &options(folder));
    assert!(done.errors.is_empty(), "{:?}", done.errors);
    (done.notes[0].clone(), done)
}

fn stem(p: &Path) -> String {
    p.file_stem().unwrap().to_string_lossy().into_owned()
}

// spec: SOC-070
#[test]
fn soc_070_a_post_note_has_in_reply_to_and_thread_id_besides_post_id_and_network() {
    let mut v = vault("post-ids");
    let (top, _) = save_post(&mut v, saved_post("10", 16, None, None), "Posts", |_| {});
    let t = text(&v, &top);
    assert_eq!((prop(&t, "post-id").as_deref(), prop(&t, "network").as_deref(), prop(&t, "thread-id").as_deref()), (Some("\"10\""), Some("mastodon"), Some("\"10\"")), "a post that starts a conversation is its own root");
    assert!(prop(&t, "in-reply-to").is_none() && prop(&t, "previous").is_none() && prop(&t, "next").is_none(), "{t}");
    // a reply: the parent and the root the network named
    let mut w = vault("post-ids-b");
    let (r, _) = save_post(&mut w, saved_post("12", 17, Some("11"), Some("10")), "Posts", |_| {});
    let t = text(&w, &r);
    assert_eq!((prop(&t, "in-reply-to").as_deref(), prop(&t, "thread-id").as_deref()), (Some("\"11\""), Some("\"10\"")));
    // no root from the network: the parent stands in
    let (r, _) = save_post(&mut w, saved_post("13", 17, Some("12"), None), "Posts", |it| it.continues = None);
    assert_eq!(prop(&text(&w, &r), "thread-id").as_deref(), Some("\"12\""));
    // a saved self-thread keeps thread / thread-ids and gets the new ids besides
    let mut s = saved_post("20", 18, None, None);
    s.thread.push(post("21", "ada", 1_793_620_800 + 17 * 86_400 + 60, Some("20")));
    let (n, _) = save_post(&mut w, s, "Posts", |_| {});
    let t = text(&w, &n);
    assert_eq!((prop(&t, "thread").as_deref(), prop(&t, "thread-ids").as_deref(), prop(&t, "thread-id").as_deref()), (Some("2"), Some("[\"20\", \"21\"]"), Some("\"20\"")));
}

// spec: SOC-071
#[test]
fn soc_071_a_post_is_matched_by_the_post_it_replies_to_the_root_and_a_self_thread_id_in_the_same_network() {
    let mut v = vault("post-match");
    let (a, _) = save_post(&mut v, saved_post("10", 16, None, None), "Posts", |_| {});
    // replies to the saved post: by in-reply-to, and by the root alone
    let mut items = post_items(vec![saved_post("11", 17, Some("10"), Some("10")), saved_post("12", 17, Some("99"), Some("10"))]);
    import::find_continues(&v, &mut items);
    for it in &items {
        let c = it.continues.as_ref().expect("a suggestion");
        assert!(c.on, "ticked");
        assert_eq!(c.anchor(), Some(a.as_path()));
    }
    // another network, an unrelated post: nothing
    let mut other = saved_post("10", 17, Some("10"), Some("10"));
    other.thread[0].network = Network::Bluesky;
    let mut items = vec![import::social_item(other, "Bluesky", false)];
    items.extend(post_items(vec![saved_post("50", 17, Some("49"), Some("48"))]));
    import::find_continues(&v, &mut items);
    assert!(items.iter().all(|i| i.continues.is_none()), "post ids are unique per network only, and no id is shared");
    // a reply to a later post of a saved self-thread matches by its thread-ids
    let mut s = saved_post("20", 18, None, None);
    s.thread.push(post("21", "ada", 1_793_620_800 + 17 * 86_400 + 60, Some("20")));
    let (st, _) = save_post(&mut v, s, "Posts", |_| {});
    let mut items = post_items(vec![saved_post("22", 19, Some("21"), Some("20"))]);
    import::find_continues(&v, &mut items);
    assert_eq!(items[0].continues.as_ref().and_then(|c| c.anchor()), Some(st.as_path()));
    // an older post: the note that replies to it is found
    save_post(&mut v, saved_post("30", 20, Some("29"), Some("28")), "Posts", |_| {});
    let mut older = post_items(vec![saved_post("29", 19, Some("28"), Some("28"))]);
    import::find_continues(&v, &mut older);
    assert!(older[0].continues.is_some(), "the note replying to this post is found");
    // the folder follows the note it continues (IMP-069)
    let (inside, _) = save_post(&mut v, saved_post("40", 16, None, None), "Deep/Posts", |_| {});
    let mut items = post_items(vec![saved_post("41", 17, Some("40"), Some("40"))]);
    import::find_continues(&v, &mut items);
    assert_eq!(import::continued_folder(&items).as_deref(), inside.parent().and_then(|p| p.to_str()));
}

// spec: SOC-072
#[test]
fn soc_072_posts_are_chained_by_time_an_older_post_asks_and_only_the_two_neighbours_change() {
    let mut v = vault("post-chain");
    let (a, _) = save_post(&mut v, saved_post("10", 16, None, None), "Posts", |_| {});
    let (c, done) = save_post(&mut v, saved_post("12", 18, Some("11"), Some("10")), "Posts", |_| {});
    // appended: it gets previous, the last note gets its next
    assert!(prop(&text(&v, &c), "previous").unwrap().contains(&stem(&a)) && prop(&text(&v, &c), "next").is_none());
    assert!(prop(&text(&v, &a), "next").unwrap().contains(&stem(&c)));
    assert_eq!(done.touched, std::slice::from_ref(&a), "one other note is rewritten");
    // an older post in the middle: the question, and Update changes the two neighbours only
    let mut items = post_items(vec![saved_post("11", 17, Some("10"), Some("10"))]);
    import::find_continues(&v, &mut items);
    let plan = items[0].continues.as_ref().unwrap().plan().unwrap();
    assert!(plan.is_older() && import::older_questions(&items)[&items[0].continues.as_ref().unwrap().group].1 == 2, "{plan:?}");
    let done = import::import(&mut v, &items, &options("Posts"));
    assert!(done.errors.is_empty(), "{:?}", done.errors);
    let b = done.notes[0].clone();
    assert_eq!(done.touched.len(), 2, "the two neighbours");
    assert!(prop(&text(&v, &b), "previous").unwrap().contains(&stem(&a)) && prop(&text(&v, &b), "next").unwrap().contains(&stem(&c)));
    assert!(prop(&text(&v, &a), "next").unwrap().contains(&stem(&b)) && prop(&text(&v, &c), "previous").unwrap().contains(&stem(&b)));
    // Leave the chain: no links, nothing else touched
    let before = (text(&v, &a), text(&v, &b), text(&v, &c));
    let (l, done) = save_post(&mut v, saved_post("9", 15, Some("10"), Some("10")), "Posts", |it| it.continues.as_mut().unwrap().update = false);
    assert!(prop(&text(&v, &l), "previous").is_none() && prop(&text(&v, &l), "next").is_none());
    assert!(done.touched.is_empty());
    assert_eq!(before, (text(&v, &a), text(&v, &b), text(&v, &c)));
    // unticked: its ids only
    let (u, done) = save_post(&mut v, saved_post("13", 19, Some("12"), Some("10")), "Posts", |it| it.continues.as_mut().unwrap().on = false);
    assert!(prop(&text(&v, &u), "thread-id").is_some() && prop(&text(&v, &u), "previous").is_none());
    assert!(done.touched.is_empty());
}

// spec: NAV-070, SOC-070
#[test]
fn nav_070_the_thread_view_lists_the_posts_of_a_conversation_and_steps_through_them_in_time_order() {
    let mut v = vault("post-overview");
    let (a, _) = save_post(&mut v, saved_post("10", 16, None, None), "Posts", |_| {});
    let (c, _) = save_post(&mut v, saved_post("12", 18, Some("11"), Some("10")), "Posts", |_| {});
    let (b, _) = save_post(&mut v, saved_post("11", 17, Some("10"), Some("10")), "Elsewhere", |it| it.continues.as_mut().unwrap().update = false);
    // the same id on another network is another thread
    let mut other = saved_post("10", 16, None, None);
    other.thread[0].network = Network::Bluesky;
    let (x, _) = save_post(&mut v, other, "Posts", |_| {});
    let idx = Index::build(&v);
    let rows = idx.thread_of(&c).unwrap();
    assert_eq!(rows.iter().map(|r| &r.rel).collect::<Vec<_>>(), vec![&a, &b, &c], "oldest first, wherever the notes are, one network");
    assert_eq!(rows[0].who, "@ada@m.test");
    assert_eq!(idx.thread_of(&x).unwrap().len(), 1);
    assert_eq!(thread::step(&rows, &a, 1), Some(b.clone()));
    assert_eq!(thread::step(&rows, &b, 1), Some(c.clone()));
    assert_eq!(thread::step(&rows, &c, 1), None);
}

/// The mails dropped at once, in the order given; the notes come back in that order too.
fn import_batch(v: &mut Vault, raws: Vec<Vec<u8>>, folder: &str, tweak: impl FnOnce(&mut Vec<Item>)) -> (Vec<Item>, Imported) {
    let mut items: Vec<Item> = raws.into_iter().map(|r| import::eml_item(r, None)).collect();
    import::find_continues(v, &mut items);
    tweak(&mut items);
    let done = import::import(v, &items, &options(folder));
    assert!(done.errors.is_empty(), "{:?}", done.errors);
    (items, done)
}

/// `previous` and `next` of a note as the stems of the notes they link to.
fn links(v: &Vault, rel: &Path) -> (Option<String>, Option<String>) {
    let t = text(v, rel);
    let stem = |k: &str| prop(&t, k).map(|s| s.trim_matches('"').trim_start_matches("[[").trim_end_matches("]]").to_string());
    (stem("previous"), stem("next"))
}

// spec: IMP-074
#[test]
fn imp_074_a_batch_of_replies_dropped_at_once_is_chained_in_date_order_with_links_both_ways() {
    let mut v = vault("batch-chain");
    // dropped newest first: the order of the drop does not matter, the date does
    let raws = vec![eml("c@h", D18, "Third", Some("b@h"), &["a@h", "b@h"]), eml("a@h", D16, "First", None, &[]), eml("b@h", D17, "Second", Some("a@h"), &["a@h"])];
    let (items, done) = import_batch(&mut v, raws, "Mail", |_| {});
    let [c, a, b] = [&done.notes[0], &done.notes[1], &done.notes[2]];
    let s = |p: &Path| Some(stem(p));
    assert_eq!(links(&v, a), (None, s(b)), "the root has no previous");
    assert_eq!(links(&v, b), (s(a), s(c)));
    assert_eq!(links(&v, c), (s(b), None), "the newest has no next");
    assert!(done.touched.is_empty(), "no note of the vault was there");
    // the root continues nothing; the replies continue an item of the import, not a note
    assert!(items[1].continues.is_none());
    let cb = items[2].continues.as_ref().unwrap();
    assert_eq!((cb.batch.as_slice(), cb.candidates.len()), (&[1][..], 0), "Second continues the item First");
    let cc = items[0].continues.as_ref().unwrap();
    assert_eq!((cc.batch.as_slice(), cc.candidates.len()), (&[2, 1][..], 0), "Third replies to Second: nearest first");
    // the Thread view lists them in the same order
    let rows = Index::build(&v).thread_of(a).unwrap();
    assert_eq!(rows.iter().map(|r| &r.rel).collect::<Vec<_>>(), vec![a, b, c]);
    // no question: nothing of the vault changes
    assert!(import::older_questions(&items).is_empty());
    // nothing of the import is a folder to follow
    assert_eq!(import::continued_folder(&items), None);
}

// spec: IMP-074
#[test]
fn imp_074_a_batch_with_a_note_of_the_vault_in_the_middle_is_one_chain_and_asks_once() {
    let mut v = vault("batch-mixed");
    let (a, _) = import_mail(&mut v, eml("a@h", D16, "Plan", None, &[]), "Mail");
    let (d, _) = import_mail(&mut v, eml("d@h", D19, "Last", Some("a@h"), &["a@h"]), "Mail");
    let raws = || vec![eml("b@h", D17, "Between", Some("a@h"), &["a@h"]), eml("c@h", D18, "Then", Some("b@h"), &["a@h", "b@h"])];
    // the question once for the thread, naming the two notes of the vault that change
    let mut found: Vec<Item> = raws().into_iter().map(|r| import::eml_item(r, None)).collect();
    import::find_continues(&v, &mut found);
    let q = import::older_questions(&found);
    assert_eq!(q.len(), 1, "once per thread, not once per item: {q:?}");
    assert_eq!(q.values().next().copied(), Some((0, 2)), "asked on the first item; a and d change");
    assert!(found.iter().all(|i| i.continues.as_ref().is_some_and(|c| c.on && c.plan().is_some())));
    // the folder is the one of the note of the vault
    assert_eq!(import::continued_folder(&found).as_deref(), Some("Mail"));
    // Update
    let before = (text(&v, &a), text(&v, &d));
    let (_, done) = import_batch(&mut v, raws(), "Mail", |_| {});
    let (b, c) = (&done.notes[0], &done.notes[1]);
    let s = |p: &Path| Some(stem(p));
    assert_eq!((links(&v, &a), links(&v, b)), ((None, s(b)), (s(&a), s(c))));
    assert_eq!((links(&v, c), links(&v, &d)), ((s(b), s(&d)), (s(c), None)));
    assert_eq!(done.touched.len(), 2);
    assert!(done.touched.contains(&a) && done.touched.contains(&d));
    // only the one line of each neighbour changed
    let strip = |t: &str, key: &str| t.lines().filter(|l| !l.starts_with(&format!("{key}:"))).collect::<Vec<_>>().join("\n");
    assert_eq!(strip(&text(&v, &a), "next"), strip(&before.0, "next"));
    assert_eq!(strip(&text(&v, &d), "previous"), strip(&before.1, "previous"));
    // Leave the chain, answered on the one item that asks: no link for the whole batch, nothing touched
    let mut w = vault("batch-mixed-leave");
    let (a2, _) = import_mail(&mut w, eml("a@h", D16, "Plan", None, &[]), "Mail");
    let (d2, _) = import_mail(&mut w, eml("d@h", D19, "Last", Some("a@h"), &["a@h"]), "Mail");
    let before = (text(&w, &a2), text(&w, &d2));
    let (_, done) = import_batch(&mut w, raws(), "Mail", |items| items[0].continues.as_mut().unwrap().update = false);
    assert!(done.touched.is_empty() && (text(&w, &a2), text(&w, &d2)) == before);
    assert!(done.notes.iter().all(|n| links(&w, n) == (None, None)), "{:?}", done.notes);
}

// spec: IMP-074
#[test]
fn imp_074_two_threads_in_one_batch_stay_separate() {
    let mut v = vault("batch-two");
    let raws = vec![eml("a@h", D16, "Alpha", None, &[]), eml("x@h", D16, "Xray", None, &[]), eml("b@h", D17, "Alpha two", Some("a@h"), &["a@h"]), eml("y@h", D17, "Xray two", Some("x@h"), &["x@h"]), eml("lone@h", D18, "Alone", None, &[])];
    let (items, done) = import_batch(&mut v, raws, "Mail", |_| {});
    let n = &done.notes;
    let s = |p: &Path| Some(stem(p));
    assert_eq!((links(&v, &n[0]), links(&v, &n[2])), ((None, s(&n[2])), (s(&n[0]), None)));
    assert_eq!((links(&v, &n[1]), links(&v, &n[3])), ((None, s(&n[3])), (s(&n[1]), None)));
    assert_eq!(links(&v, &n[4]), (None, None), "a mail of no thread is chained to nothing");
    assert!(items[4].continues.is_none());
    let (g1, g2) = (items[2].continues.as_ref().unwrap().group, items[3].continues.as_ref().unwrap().group);
    assert_ne!(g1, g2, "two threads");
}

// spec: IMP-074
#[test]
fn imp_074_a_ticked_off_item_is_left_out_of_the_chain_and_the_rest_still_links() {
    let mut v = vault("batch-off");
    let raws = vec![eml("a@h", D16, "First", None, &[]), eml("b@h", D17, "Second", Some("a@h"), &["a@h"]), eml("c@h", D18, "Third", Some("b@h"), &["a@h", "b@h"])];
    let (_, done) = import_batch(&mut v, raws, "Mail", |items| items[1].continues.as_mut().unwrap().on = false);
    let (a, b, c) = (&done.notes[0], &done.notes[1], &done.notes[2]);
    let s = |p: &Path| Some(stem(p));
    assert_eq!(links(&v, b), (None, None), "unticked: its ids only");
    assert_eq!((links(&v, a), links(&v, c)), ((None, s(c)), (s(a), None)));
}

// spec: IMP-074, SOC-072
#[test]
fn imp_074_the_posts_of_one_conversation_dropped_at_once_are_chained_per_network() {
    let mut v = vault("batch-posts");
    let mut other = saved_post("10", 16, None, None);
    other.thread[0].network = Network::Bluesky;
    let mut items = post_items(vec![saved_post("12", 18, Some("11"), Some("10")), saved_post("10", 16, None, None), saved_post("11", 17, Some("10"), Some("10"))]);
    items.push(import::social_item(other, "Bluesky", false));
    import::find_continues(&v, &mut items);
    let done = import::import(&mut v, &items, &options("Posts"));
    assert!(done.errors.is_empty(), "{:?}", done.errors);
    let [c, a, b, x] = [&done.notes[0], &done.notes[1], &done.notes[2], &done.notes[3]];
    let s = |p: &Path| Some(stem(p));
    assert_eq!((links(&v, a), links(&v, b), links(&v, c)), ((None, s(b)), (s(a), s(c)), (s(b), None)));
    assert_eq!(links(&v, x), (None, None), "the same id on another network is another thread");
    assert!(items[3].continues.is_none(), "it matches no post of the other network");
}
