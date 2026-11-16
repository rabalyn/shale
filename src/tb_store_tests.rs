//! `tb_store` against generated Thunderbird profiles (`tb_fixture_tests`, DEV-004): finding profiles and
//! accounts, local mbox and maildir folders, nested and non-ASCII folders, flags, the index, large
//! and broken files and the errors. They are children of `tb_store`, so they can reach its helpers.

use super::*;
use crate::tb_fixture_tests::{Kind, Msg, TbFixture};

fn find(fx: &TbFixture, link: &str, name: Option<&str>) -> Result<Vec<Vec<u8>>, String> {
    find_in(std::slice::from_ref(&fx.root), &[link.to_string()], name)
}

fn text(raw: &[u8]) -> String {
    String::from_utf8_lossy(raw).into_owned()
}

fn local(fx: &TbFixture) -> PathBuf {
    fx.account("server1", Kind::Local, "Local Folders", "nobody", "Mail/Local Folders")
}

// spec: IMP-018
#[test]
fn profiles_are_the_folders_with_a_prefs_file_and_the_root_itself() {
    let fx = TbFixture::new("profiles");
    let second = fx.add_profile("zzz99999.work");
    std::fs::create_dir_all(fx.root.join("Crash Reports")).unwrap();
    let mut got = profiles(std::slice::from_ref(&fx.root));
    got.sort();
    assert_eq!(got, vec![fx.profile.clone(), second], "the folder without prefs.js is not a profile");
    // a root that is a profile itself (an old layout) is one too
    std::fs::write(fx.root.join("prefs.js"), "").unwrap();
    assert!(profiles(std::slice::from_ref(&fx.root)).contains(&fx.root));
    // a root that is missing has none
    assert!(profiles(&[fx.root.join("nope")]).is_empty());
    assert!(profiles(&[]).is_empty());
}

// spec: IMP-018
#[test]
fn accounts_come_from_prefs_with_host_user_kind_and_folder() {
    let fx = TbFixture::new("accounts");
    let local = local(&fx);
    let imap = fx.account("server2", Kind::Imap, "imap.uni.example", "ada", "ImapMail/imap.uni.example");
    let pop = fx.account("server3", Kind::Pop, "pop.uni.example", "ada", "Mail/pop.uni.example");
    fx.prefs_line("user_pref(\"mail.server.server4.type\", \"imap\");"); // no host: dropped
    fx.prefs_line("user_pref(\"mail.server.server5.hostname\", \"abs.example\");");
    fx.prefs_line("user_pref(\"mail.server.server5.directory\", \"/somewhere/abs\");");
    fx.prefs_line("user_pref(\"mail.server.server6.hostname\", \"gone.example\");");
    fx.prefs_line("user_pref(\"mail.server.server6.directory-rel\", \"[ProfD]Mail/gone\");");
    fx.prefs_line("user_pref(\"mail.server.noise\", 1);"); // not a field
    fx.prefs_line("// a comment");
    let s = servers(&fx.profile);
    let by = |h: &str| s.iter().find(|s| s.host == h).unwrap_or_else(|| panic!("{h} missing in {s:?}")).clone();
    assert_eq!(s.len(), 5, "{s:?}");
    assert_eq!((by("Local Folders").user.as_str(), by("Local Folders").kind.as_str(), by("Local Folders").dir), ("nobody", "none", local));
    assert_eq!((by("imap.uni.example").kind.as_str(), by("imap.uni.example").dir), ("imap", imap));
    assert_eq!((by("pop.uni.example").kind.as_str(), by("pop.uni.example").dir), ("pop3", pop));
    assert_eq!(by("abs.example").dir, PathBuf::from("/somewhere/abs"));
    assert_eq!(by("gone.example").dir, fx.profile.join("Mail/gone"));
    // no prefs.js: no accounts
    assert!(servers(&fx.root.join("missing")).is_empty());
}

// spec: IMP-018
#[test]
fn a_local_folder_is_read_at_the_position_in_the_link_with_its_flags_and_unescaped_from_lines() {
    let fx = TbFixture::new("local");
    let acc = local(&fx);
    let msgs = [Msg::new("Erste", "eins"), Msg::new("Hörsaal B – Pläne", "From the architects\nFrom here on").status(0x0003), Msg::new("Dritte", "drei")];
    let (_, at) = fx.mbox(&acc, &["Inbox"], &msgs);
    for (i, m) in msgs.iter().enumerate() {
        let got = find(&fx, &format!("mailbox-message://nobody@Local%20Folders/Inbox#{}", at[i]), None).unwrap();
        assert_eq!(got.len(), 1);
        let t = text(&got[0]);
        assert!(t.starts_with(&format!("X-Mozilla-Status: {:04x}\n", m.status)), "the flags are kept: {t}");
        assert!(t.contains(&format!("Subject: {}\n", m.subject)), "{t}");
        for (j, other) in msgs.iter().enumerate() {
            assert_eq!(t.contains(&format!("Subject: {}\n", other.subject)), i == j, "only message {i}: {t}");
        }
    }
    let t = text(&find(&fx, &format!("mailbox-message://nobody@Local%20Folders/Inbox#{}", at[1]), None).unwrap()[0]);
    assert!(t.contains("\nFrom the architects\nFrom here on"), "the >From escape is undone: {t}");
}

// spec: IMP-018
#[test]
fn nested_folders_and_names_with_non_ascii_letters_are_found() {
    let fx = TbFixture::new("nested");
    let acc = local(&fx);
    let (file, at) = fx.mbox(&acc, &["Inbox", "Projekte", "Größe & Maße"], &[Msg::new("Tief", "unten")]);
    assert!(file.ends_with("Inbox.sbd/Projekte.sbd/Größe & Maße"), "{}", file.display());
    let link = format!("mailbox-message://nobody@Local%20Folders/Inbox/Projekte/Gr%C3%B6%C3%9Fe%20%26%20Ma%C3%9Fe#{}", at[0]);
    assert!(text(&find(&fx, &link, None).unwrap()[0]).contains("unten"));
    // case of the names does not matter (Thunderbird writes "Inbox", links say "INBOX")
    let link = format!("mailbox-message://nobody@Local%20Folders/INBOX/projekte/gr%C3%B6%C3%9Fe%20%26%20ma%C3%9Fe#{}", at[0]);
    assert!(text(&find(&fx, &link, None).unwrap()[0]).contains("unten"));
}

// spec: IMP-018
#[test]
fn imap_folders_with_modified_utf7_names_are_found_by_the_imap_link() {
    let fx = TbFixture::new("utf7");
    let acc = fx.account("server2", Kind::Imap, "imap.uni.example", "ada", "ImapMail/imap.uni.example");
    // Thunderbird names the folders of IMAP accounts as the server does: modified UTF-7
    fx.mbox(&acc, &["INBOX", "Entw&APw-rfe"], &[Msg::new("Entwurf zum Bau", "Text")]);
    let got = find(&fx, "imap://ada@imap.uni.example:993/fetch%3EUID%3E/INBOX/Entw&APw-rfe%3E12", Some("Entwurf zum Bau.eml")).unwrap();
    assert_eq!(got.len(), 1);
    assert!(text(&got[0]).contains("Text"));
}

// spec: IMP-018
#[test]
fn a_message_deleted_but_not_compacted_is_still_read_at_its_position_with_its_flag() {
    let fx = TbFixture::new("deleted");
    let acc = local(&fx);
    let (_, at) = fx.mbox(&acc, &["Inbox"], &[Msg::new("Alt", "weg").status(0x0009), Msg::new("Neu", "da")]);
    let t = text(&find(&fx, &format!("mailbox-message://nobody@Local%20Folders/Inbox#{}", at[0]), None).unwrap()[0]);
    assert!(t.starts_with("X-Mozilla-Status: 0009") && t.contains("weg") && !t.contains("Neu"), "{t}");
}

// spec: IMP-018
#[test]
fn a_large_message_is_read_whole_and_the_next_one_is_not_swallowed() {
    let fx = TbFixture::new("large");
    let acc = local(&fx);
    let body: String = (0..300_000).map(|i| format!("line {i} of a very large mail\n")).collect();
    assert!(body.len() > 8 << 20);
    let (_, at) = fx.mbox(&acc, &["Inbox"], &[Msg::new("Davor", "kurz"), Msg::new("Riesig", &body), Msg::new("Danach", "kurz")]);
    let big = find(&fx, &format!("mailbox-message://nobody@Local%20Folders/Inbox#{}", at[1]), None).unwrap().remove(0);
    let t = text(&big);
    assert!(big.len() > body.len() && t.contains("line 299999 of a very large mail") && !t.contains("Danach") && !t.contains("Davor"));
    let last = find(&fx, &format!("mailbox-message://nobody@Local%20Folders/Inbox#{}", at[2]), None).unwrap();
    assert!(text(&last[0]).contains("Subject: Danach"));
}

// spec: IMP-018
#[test]
fn maildir_folders_are_read_by_their_index_and_by_the_subject() {
    let fx = TbFixture::new("maildir");
    let acc = local(&fx);
    let msgs = [Msg::new("Erste Post", "eins").id("m1@x"), Msg::new("Zweite Post", "zwei").id("m2@x")];
    let (dir, names) = fx.maildir(&acc, &["Inbox", "Projekte"], &msgs);
    // by the index: the row's storeToken is the file name
    fx.index(&dir, &[(7, "Erste Post", "m1@x", &names[0]), (8, "Zweite Post", "m2@x", &names[1])]);
    let got = find(&fx, "mailbox-message://nobody@Local%20Folders/Inbox/Projekte#8", None).unwrap();
    assert_eq!(got.len(), 1);
    assert!(text(&got[0]).contains("zwei") && text(&got[0]).contains("Subject: Zweite Post"));
    // a token that points at the wrong file is not trusted: found by Message-ID
    fx.index(&dir, &[(8, "Zweite Post", "m2@x", &names[0])]);
    let got = find(&fx, "mailbox-message://nobody@Local%20Folders/Inbox/Projekte#8", None).unwrap();
    assert!(text(&got[0]).contains("zwei"));
    // no index at all: by the subject of the file name
    std::fs::remove_file(index_of(&dir)).unwrap();
    let got = find(&fx, "mailbox-message://nobody@Local%20Folders/Inbox/Projekte#8", Some("Erste Post.eml")).unwrap();
    assert!(text(&got[0]).contains("eins"));
    // the maildir files in new/ count too, and a message there is found by Message-ID alone
    std::fs::rename(dir.join("cur").join(&names[1]), dir.join("new").join(&names[1])).unwrap();
    fx.index(&dir, &[(9, "Zweite Post", "m2@x", "unknown-token")]);
    let got = find(&fx, "mailbox-message://nobody@Local%20Folders/Inbox/Projekte#9", None).unwrap();
    assert!(text(&got[0]).contains("zwei"));
}

// spec: IMP-018
#[test]
fn the_index_finds_imap_messages_and_says_when_their_text_is_not_stored() {
    let fx = TbFixture::new("index");
    let acc = fx.account("server2", Kind::Imap, "imap.uni.example", "ada", "ImapMail/imap.uni.example");
    let msgs = [Msg::new("Plan (Stand 4)", "plan").id("p@x"), Msg::new("Anderes", "anders").id("a@x")];
    let (file, at) = fx.mbox(&acc, &["INBOX", "Bau"], &msgs);
    let link = |uid: u32| format!("imap-message://ada@imap.uni.example/INBOX/Bau#{uid}");
    fx.index(&file, &[(40, "Plan (Stand 4)", "p@x", &at[0].to_string()), (41, "Anderes", "a@x", &at[1].to_string()), (42, "Nur Kopfzeilen", "n@x", "999999")]);
    assert!(text(&find(&fx, &link(41), None).unwrap()[0]).contains("anders"));
    assert!(text(&find(&fx, &link(40), None).unwrap()[0]).contains("plan"));
    // a row whose text is not in the folder file (not downloaded): the explanation
    let err = find(&fx, &link(42), None).unwrap_err();
    assert!(err.contains("not stored on this computer") && err.contains("Keep messages"), "{err}");
    // a UID the index does not know
    let err = find(&fx, &link(43), None).unwrap_err();
    assert!(err.contains("not in Thunderbird's index"), "{err}");
}

// spec: IMP-018
#[test]
fn accounts_are_matched_by_host_kind_and_user() {
    let fx = TbFixture::new("match");
    let a = fx.account("server1", Kind::Imap, "imap.uni.example", "ada", "ImapMail/ada");
    let b = fx.account("server2", Kind::Imap, "imap.uni.example", "bob", "ImapMail/bob");
    fx.account("server3", Kind::Pop, "imap.uni.example", "ada", "Mail/pop");
    let r = |link: &str| server_dirs(std::slice::from_ref(&fx.root), &parse_link(link).unwrap());
    assert_eq!(r("imap-message://ada@IMAP.uni.example/INBOX#1"), vec![a.clone(), b.clone()], "the user's account first, the other as a fallback");
    assert_eq!(r("imap-message://bob@imap.uni.example/INBOX#1")[0], b);
    assert_eq!(r("imap-message://imap.uni.example/INBOX#1"), vec![a, b], "no user in the link: all of that host");
    assert!(r("imap-message://ada@elsewhere.example/INBOX#1").is_empty());
}

// spec: IMP-018
#[test]
fn without_usable_prefs_the_usual_folder_names_are_tried() {
    let fx = TbFixture::new("noprefs");
    std::fs::remove_file(fx.profile.join("prefs.js")).unwrap();
    std::fs::write(fx.profile.join("prefs.js"), "garbage without prefs\n").unwrap();
    let acc = fx.profile.join("ImapMail/imap.uni.example-1");
    std::fs::create_dir_all(&acc).unwrap();
    std::fs::create_dir_all(fx.profile.join("ImapMail/imap.other.example")).unwrap();
    fx.mbox(&acc, &["INBOX"], &[Msg::new("Ohne Einstellungen", "gefunden")]);
    let got = find(&fx, "imap-message://ada@imap.uni.example/INBOX#5", Some("Ohne Einstellungen.eml")).unwrap();
    assert!(text(&got[0]).contains("gefunden"));
    // and a local account's folder "Mail/<host>"
    let local = fx.profile.join("Mail/Local Folders");
    let (_, at) = fx.mbox(&local, &["Inbox"], &[Msg::new("Lokal", "hier")]);
    assert!(text(&find(&fx, &format!("mailbox-message://nobody@Local%20Folders/Inbox#{}", at[0]), None).unwrap()[0]).contains("hier"));
}

// spec: IMP-018
#[test]
fn a_mailbox_link_names_the_folder_file_directly_and_other_profiles_are_searched() {
    let fx = TbFixture::new("direct");
    let other = fx.add_profile("zzz99999.work");
    let acc = fx.account_in(&other, "server1", Kind::Local, "Local Folders", "nobody", "Mail/Local Folders");
    let msgs = [Msg::new("Direkt", "im zweiten Profil")];
    let (file, at) = fx.mbox(&acc, &["Inbox"], &msgs);
    // through the profile's prefs (the first profile has no such account)
    let got = find(&fx, &format!("mailbox-message://nobody@Local%20Folders/Inbox#{}", at[0]), None).unwrap();
    assert!(text(&got[0]).contains("im zweiten Profil"));
    // through the file path in the link, no profile needed
    let link = format!("mailbox://{}?number={}", file.display(), at[0]);
    let got = find_in(&[fx.root.join("nowhere")], &[link], None).unwrap();
    assert!(text(&got[0]).contains("im zweiten Profil"));
}

// spec: IMP-018
#[test]
fn a_compacted_folder_or_a_stale_position_falls_back_to_the_subject() {
    let fx = TbFixture::new("stale");
    let acc = local(&fx);
    fx.mbox(&acc, &["Inbox"], &[Msg::new("Verschoben", "nach dem Kompaktieren")]);
    // the position points into the middle of a line of the body: not a message start
    let got = find(&fx, "mailbox-message://nobody@Local%20Folders/Inbox#150", Some("Verschoben.eml"));
    assert!(text(&got.unwrap()[0]).contains("nach dem Kompaktieren"));
    // a position behind the end of the file
    let got = find(&fx, "mailbox-message://nobody@Local%20Folders/Inbox#99999999", Some("Verschoben.eml"));
    assert_eq!(got.unwrap().len(), 1);
}

// spec: IMP-018
#[test]
fn broken_folder_files_and_indexes_give_errors_and_never_a_crash() {
    let fx = TbFixture::new("broken");
    let acc = local(&fx);
    let (file, at) = fx.mbox(&acc, &["Inbox"], &[Msg::new("Heil", "ganz")]);
    let link = format!("mailbox-message://nobody@Local%20Folders/Inbox#{}", at[0]);
    let junk: Vec<u8> = (0..4000u32).map(|i| (i * 7 % 251) as u8).map(|b| if b == b':' { b'.' } else { b }).collect();
    // a folder file of binary garbage
    std::fs::write(&file, &junk).unwrap();
    let err = find(&fx, &link, Some("Heil.eml")).unwrap_err();
    assert!(err.contains("the message was not found") && err.contains("Heil.eml"), "{err}");
    // an empty one
    std::fs::write(&file, "").unwrap();
    assert!(find(&fx, &link, Some("Heil.eml")).unwrap_err().contains("not found"));
    // a message cut off in the middle of the headers (no body, no end of line)
    std::fs::write(&file, "From - Thu Nov 12 10:00:00 2026\nSubject: Heil\nMessage-ID: <h@x>").unwrap();
    assert!(text(&find(&fx, &link, None).unwrap()[0]).contains("Message-ID: <h@x>"));
    // a broken index: garbage, and one cut off in a row
    std::fs::write(index_of(&file), &junk).unwrap();
    assert!(find(&fx, &format!("mailbox-message://nobody@Local%20Folders/Inbox#{}", at[0] + 5000), None).unwrap_err().contains("not in Thunderbird's index"));
    std::fs::write(index_of(&file), "< <(a=c)>(81=subject)>\n[1F(^81=Heil").unwrap();
    assert!(find(&fx, "imap-message://nobody@Local%20Folders/Inbox#31", None).is_err());
    let rows = mork_rows(b"[1F(^81=Heil");
    assert!(rows.len() <= 1);
}

// spec: IMP-018
#[test]
fn errors_say_what_is_missing() {
    let fx = TbFixture::new("errors");
    let acc = local(&fx);
    fx.mbox(&acc, &["Inbox"], &[Msg::new("Da", "da")]);
    // no readable link
    let err = find_in(std::slice::from_ref(&fx.root), &["https://example.org/x".into()], None).unwrap_err();
    assert!(err.contains("could not be read") && err.contains("https://example.org/x"), "{err}");
    assert!(find_in(std::slice::from_ref(&fx.root), &[], None).is_err());
    // no profile folder at all
    let err = find_in(&[], &["mailbox-message://nobody@Local%20Folders/Inbox#0".into()], None).unwrap_err();
    assert!(err.contains("profile folder was not found"), "{err}");
    // a root that does not exist: no folder for the link
    let err = find_in(&[fx.root.join("missing")], &["mailbox-message://nobody@Local%20Folders/Inbox#0".into()], None).unwrap_err();
    assert!(err.contains("no Thunderbird folder for “Inbox”"), "{err}");
    // the account exists, the folder does not
    let err = find(&fx, "mailbox-message://nobody@Local%20Folders/Archiv/2025#0", None).unwrap_err();
    assert!(err.contains("no folder “Archiv/2025”"), "{err}");
    // a local link without a hint about IMAP; an IMAP link with it
    assert!(!find(&fx, "mailbox-message://nobody@Local%20Folders/Inbox#99999", Some("Nope.eml")).unwrap_err().contains("Keep messages"));
    // a file name that is empty after cleaning is not a subject
    assert!(find(&fx, "mailbox-message://nobody@Local%20Folders/Inbox#99999", Some(" .eml ")).is_err());
}

// spec: IMP-018
#[test]
#[cfg(unix)]
fn a_folder_file_that_cannot_be_read_is_an_error_not_a_crash() {
    use std::os::unix::fs::PermissionsExt;
    let fx = TbFixture::new("unreadable");
    let acc = local(&fx);
    let (file, at) = fx.mbox(&acc, &["Inbox"], &[Msg::new("Gesperrt", "zu")]);
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::File::open(&file).is_ok() {
        return; // running as root: the permission does not stop anything
    }
    let err = find(&fx, &format!("mailbox-message://nobody@Local%20Folders/Inbox#{}", at[0]), Some("Gesperrt.eml")).unwrap_err();
    assert!(err.contains("the message was not found"), "{err}");
    // an unreadable prefs.js: no accounts, the usual folders are tried
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::fs::set_permissions(fx.profile.join("prefs.js"), std::fs::Permissions::from_mode(0o000)).unwrap();
    assert!(servers(&fx.profile).is_empty());
    std::fs::set_permissions(fx.profile.join("prefs.js"), std::fs::Permissions::from_mode(0o600)).unwrap();
}

// spec: IMP-018
#[test]
fn message_links_of_every_kind_and_their_edge_cases_are_read() {
    // a port, a user without host part, escapes that are not escapes
    let r = parse_link(" imap-message://a%2Bb@host.example:143/INBOX/100%/x#7 ").unwrap();
    assert_eq!((r.user.as_str(), r.host.as_str(), r.folder.clone(), r.key), ("a+b", "host.example", vec!["INBOX".to_string(), "100%".into(), "x".into()], Some(7)));
    // no user, no key, a query instead of a fragment
    let r = parse_link("mailbox-message://Local%20Folders/Inbox?number=12").unwrap();
    assert_eq!((r.user.as_str(), r.host.as_str(), r.key), ("", "Local Folders", Some(12)));
    let r = parse_link("mailbox-message://h/Inbox").unwrap();
    assert_eq!(r.key, None);
    // an authority without a path
    assert_eq!(parse_link("imap-message://h").unwrap().folder, Vec::<String>::new());
    // imap links need "UID>" and a message number
    assert!(parse_link("imap://h/INBOX").is_none());
    assert!(parse_link("imap://h/fetch%3EUID%3E").is_none());
    assert!(parse_link("not a link").is_none());
    assert!(parse_link("ftp://h/x").is_none());
    // a mailbox link without a number
    assert_eq!(parse_link("mailbox:///tmp/x/Inbox").unwrap().key, None);
    assert_eq!(pct("%zz%4"), "%zz%4");
    assert_eq!(digits("12abc"), Some(12));
    assert_eq!(digits("abc"), None);
}

// spec: IMP-018
#[test]
fn the_fixture_writes_what_thunderbird_writes() {
    let fx = TbFixture::new("shape");
    let acc = local(&fx);
    let (file, at) = fx.mbox(&acc, &["Inbox", "Sub"], &[Msg::new("A", "x"), Msg::new("B", "y")]);
    assert!(fx.root.join("profiles.ini").is_file() && fx.profile.join("prefs.js").is_file());
    assert!(acc.join("Inbox").is_file() && acc.join("Inbox.sbd").is_dir() && file.is_file());
    assert_eq!(at[0], 0);
    let all = std::fs::read_to_string(&file).unwrap();
    assert!(all[at[1] as usize..].starts_with("From - "));
    let one = Msg::new("Mail", "body").maildir_text();
    assert!(!one.starts_with("From ") && one.starts_with("X-Mozilla-Status"));
}

fn imap_link(folder: &str) -> String {
    format!("imap-message://ada@imap.uni.example/INBOX/{folder}#77")
}

fn imap(fx: &TbFixture) -> PathBuf {
    fx.account("server2", Kind::Imap, "imap.uni.example", "ada", "ImapMail/imap.uni.example")
}

const DELETED: &str = "the message was deleted in Thunderbird (Thunderbird has not compacted the folder yet)";

// spec: IMP-030
#[test]
fn imp_030_a_search_by_subject_skips_deleted_copies_in_mbox_and_maildir_folders() {
    let fx = TbFixture::new("del-subject");
    let acc = imap(&fx);
    // only a deleted copy: not found
    fx.mbox(&acc, &["INBOX", "Alt"], &[Msg::new("Nur weg", "tot").status(0x0009)]);
    let err = find(&fx, &imap_link("Alt"), Some("Nur weg.eml")).unwrap_err();
    assert!(err.contains(DELETED), "{err}");
    // a live and a deleted copy of the same message: the live one is found, the deleted one is not offered
    fx.mbox(&acc, &["INBOX", "Paar"], &[Msg::new("Paar", "tot").status(0x0009), Msg::new("Paar", "lebt"), Msg::new("Paar", "tot2").status(0x0108)]);
    let got = find(&fx, &imap_link("Paar"), Some("Paar.eml")).unwrap();
    assert_eq!(got.len(), 1);
    assert!(text(&got[0]).contains("lebt"));
    // the flag is only the 0x0008 bit: read, replied and flagged are live
    fx.mbox(&acc, &["INBOX", "Flags"], &[Msg::new("Flags", "a").status(0x0003), Msg::new("Flags", "b").status(0x0010)]);
    assert_eq!(find(&fx, &imap_link("Flags"), Some("Flags.eml")).unwrap().len(), 2);
    // maildir folders the same
    fx.maildir(&acc, &["INBOX", "Md"], &[Msg::new("Md", "tot").status(0x0009), Msg::new("Md", "lebt")]);
    let got = find(&fx, &imap_link("Md"), Some("Md.eml")).unwrap();
    assert_eq!(got.len(), 1);
    assert!(text(&got[0]).contains("lebt"));
    fx.maildir(&acc, &["INBOX", "MdAlt"], &[Msg::new("MdAlt", "tot").status(0x0008)]);
    assert!(find(&fx, &imap_link("MdAlt"), Some("MdAlt.eml")).unwrap_err().contains(DELETED));
}

// spec: IMP-030
#[test]
fn imp_030_a_search_by_message_id_after_a_stale_position_skips_deleted_copies() {
    let fx = TbFixture::new("del-id");
    let acc = imap(&fx);
    let dead = Msg::new("Gleich", "tot").status(0x0009).id("same@x");
    let live = Msg::new("Gleich", "lebt").id("same@x");
    let (file, _) = fx.mbox(&acc, &["INBOX", "Id"], &[dead.clone(), live]);
    // the index points at a stale position (3): the Message-ID decides, and only the live copy counts
    fx.index(&file, &[(77, "Gleich", "same@x", "3")]);
    let got = find(&fx, &imap_link("Id"), None).unwrap();
    assert_eq!(got.len(), 1);
    assert!(text(&got[0]).contains("lebt"));
    // only the deleted copy: not found, and the error says so
    let (file, _) = fx.mbox(&acc, &["INBOX", "Id2"], &[dead]);
    fx.index(&file, &[(77, "Gleich", "same@x", "3")]);
    let err = find(&fx, &imap_link("Id2"), None).unwrap_err();
    assert!(err.contains(DELETED) && !err.contains("not stored on this computer"), "{err}");
    // the same through a maildir folder
    let (dir, _) = fx.maildir(&acc, &["INBOX", "Id3"], &[Msg::new("Gleich", "tot").status(0x0009).id("same@x"), Msg::new("Gleich", "lebt").id("same@x")]);
    fx.index(&dir, &[(77, "Gleich", "same@x", "nope")]);
    let got = find(&fx, &imap_link("Id3"), None).unwrap();
    assert_eq!(got.len(), 1);
    assert!(text(&got[0]).contains("lebt"));
    // only a deleted copy in a maildir folder, found by its Message-ID
    let (dir, _) = fx.maildir(&acc, &["INBOX", "Id4"], &[Msg::new("Gleich", "tot").status(0x0008).id("same@x")]);
    fx.index(&dir, &[(77, "Gleich", "same@x", "nope")]);
    assert!(find(&fx, &imap_link("Id4"), None).unwrap_err().contains(DELETED));
}

// spec: IMP-030
#[test]
fn imp_030_the_generic_error_stays_when_no_deleted_copy_matched() {
    let fx = TbFixture::new("del-generic");
    let acc = imap(&fx);
    // nothing matches at all
    fx.mbox(&acc, &["INBOX", "Leer"], &[Msg::new("Anderes", "x")]);
    let err = find(&fx, &imap_link("Leer"), Some("Gesucht.eml")).unwrap_err();
    assert!(err.contains("the message was not found") && !err.contains(DELETED), "{err}");
    // a deleted copy of a DIFFERENT message next to no match: still generic (mbox and maildir)
    fx.mbox(&acc, &["INBOX", "Fremd"], &[Msg::new("Anderes", "weg").status(0x0009)]);
    let err = find(&fx, &imap_link("Fremd"), Some("Gesucht.eml")).unwrap_err();
    assert!(err.contains("the message was not found") && !err.contains(DELETED), "{err}");
    fx.maildir(&acc, &["INBOX", "FremdMd"], &[Msg::new("Anderes", "weg").status(0x0009)]);
    let err = find(&fx, &imap_link("FremdMd"), Some("Gesucht.eml")).unwrap_err();
    assert!(err.contains("the message was not found") && !err.contains(DELETED), "{err}");
    // the Message-ID path: a deleted copy with another Message-ID does not count
    let (file, _) = fx.mbox(&acc, &["INBOX", "FremdId"], &[Msg::new("Anderes", "weg").status(0x0009).id("other@x")]);
    fx.index(&file, &[(77, "Gesucht", "wanted@x", "3")]);
    let err = find(&fx, &imap_link("FremdId"), None).unwrap_err();
    assert!(!err.contains(DELETED), "{err}");
}

// spec: IMP-030
#[test]
fn imp_030_reading_by_a_known_position_still_returns_a_deleted_copy() {
    let fx = TbFixture::new("del-known");
    let acc = local(&fx);
    let (file, at) = fx.mbox(&acc, &["Inbox"], &[Msg::new("Alt", "weg").status(0x0008)]);
    assert!(text(&read_at(&file, at[0]).unwrap()).contains("weg"));
    assert!(is_deleted("X-Mozilla-Status: 0008\nSubject: x") && is_deleted("X-Mozilla-Status: 000F\n") && !is_deleted("X-Mozilla-Status: 0001\n"));
    assert!(!is_deleted("Subject: x") && !is_deleted("X-Mozilla-Status: zz\n"));
}

// spec: IMP-030
#[test]
fn imp_030_a_position_must_start_with_a_real_header_line_or_the_from_line() {
    let fx = TbFixture::new("header-check");
    let acc = local(&fx);
    let ok = Msg::new("Echt", "inhalt");
    // a message whose first header is not X-Mozilla-Status
    let other = "From - Thu Nov 12 10:00:00 2026\nSubject: Ohne Status\n\nbody\n\n".to_string();
    let bare = "Return-Path: <a@b>\nSubject: Ohne From-Zeile\n\nbody\n\n".to_string();
    let garbage = ["not a header: just text with a colon\n", "see http://example.org/x\n", ": no name\n", "\u{e4}: non-ascii name\n", "\tfolded: line\n", "\n"];
    let mut blocks = vec![ok.mbox_text(), other, bare];
    blocks.extend(garbage.iter().map(|g| format!("{g}rest\n")));
    let (file, at) = fx.mbox_raw(&acc, &["Inbox"], &blocks);
    assert!(text(&read_at(&file, at[0]).unwrap()).starts_with("X-Mozilla-Status: 0001"), "a normal message is still read");
    assert!(text(&read_at(&file, at[1]).unwrap()).contains("Ohne Status"));
    assert!(text(&read_at(&file, at[2]).unwrap()).contains("Ohne From-Zeile"));
    for (i, g) in garbage.iter().enumerate() {
        assert!(read_at(&file, at[3 + i]).is_none(), "{g:?} is not the start of a message");
    }
    assert!(is_header_line(b"X-Mozilla-Status: 0001\n") && is_header_line(b"a:b") && !is_header_line(b"a b: c") && !is_header_line(b"no colon"));
    // a link to a garbage position falls back to the subject
    let (_, at) = fx.mbox_raw(&acc, &["Inbox2"], &["Subject: x\n\n".to_string(), "ein Satz: im Text\n".to_string()]);
    assert!(find(&fx, &format!("mailbox-message://nobody@Local%20Folders/Inbox2#{}", at[1]), None).is_err());
}
