//! The compose box extras and polls on cards (#589, SOC-061 .. SOC-067): a poll, a time to send,
//! the language and a video, and the poll a card shows. The window runs headless like in
//! `ui_tests.rs`; what goes over the wire is checked against mock servers in `mastodon.rs`,
//! `bluesky.rs` and `video.rs`.

use super::actions::InstanceInfo;
use super::poll::{DraftPoll, Poll, PollLimits, PollOption};
use super::schedule::{ScheduleInput, Scheduled, MIN_AHEAD};
use super::ui_tests::{home, wait_for, W};
use super::write_tests::{compose, ctrl_enter, send_it, toast_has, type_text};
use super::Network;
use eframe::egui::Event;

fn limits() -> InstanceInfo {
    InstanceInfo { poll: PollLimits { max_options: 3, max_chars: 10, min_expiry: 600, max_expiry: 7 * 86_400 }, ..InstanceInfo::defaults(Network::Mastodon) }
}

fn open_box(w: &mut W, info: InstanceInfo) {
    *w.script.info.lock().unwrap() = Some(info);
    w.click("new-post");
    let acct = home().account;
    wait_for(w, |w| w.app.social.writes.info.contains_key(&acct));
    w.frames(2);
}

fn type_into(w: &mut W, key: &str, s: &str) {
    w.click(key);
    w.frame(vec![Event::Text(s.into())]);
    w.frames(2);
}

fn poll(voted: bool, expired: bool) -> Poll {
    Poll {
        id: "p1".into(),
        options: vec![PollOption { title: "Tea".into(), votes: Some(3) }, PollOption { title: "Coffee".into(), votes: Some(1) }, PollOption { title: "Water".into(), votes: Some(0) }],
        expires_at: Some(crate::calendar::time::now() + 7200),
        expired,
        multiple: false,
        votes_count: 4,
        voters_count: Some(4),
        voted,
        own_votes: if voted { vec![1] } else { vec![] },
    }
}

fn with_poll(w: &mut W, p: Poll) {
    let key = (home().account, "home".to_string());
    w.app.social.timelines.get_mut(&key).unwrap().posts[0].poll = Some(p);
    w.frames(3);
}

fn first_poll(w: &W) -> Poll {
    let key = (home().account, "home".to_string());
    w.app.social.timelines[&key].posts[0].poll.clone().unwrap()
}

// spec: SOC-061
#[test]
fn soc_061_a_poll_has_two_options_up_to_the_servers_limit_a_duration_and_two_switches_and_goes_out_with_the_post() {
    let mut w = W::new("poll-box");
    open_box(&mut w, limits());
    type_text(&mut w, "Tea or coffee?");
    w.click("compose:add-poll");
    assert_eq!(compose(&w).poll.as_ref().unwrap().options.len(), 2);
    let info = w.app.social.info_for(&w.app.cfg.social_accounts[0].account);
    assert_eq!(compose(&w).problem(Network::Mastodon, &info).as_deref(), Some("A poll needs at least two options"));
    type_into(&mut w, "compose:poll-option:0", "Tea");
    type_into(&mut w, "compose:poll-option:1", "Coffee");
    assert_eq!(compose(&w).problem(Network::Mastodon, &info), None);
    // the third option is the last the server allows; a fourth is not offered
    w.click("compose:poll-add-option");
    assert_eq!(compose(&w).poll.as_ref().unwrap().options.len(), 3);
    w.click("compose:poll-add-option");
    assert_eq!(compose(&w).poll.as_ref().unwrap().options.len(), 3, "at most what the server allows");
    // an empty third option is left out; one that is too long for the server is named
    assert_eq!(compose(&w).poll.as_ref().unwrap().draft().options, ["Tea", "Coffee"]);
    w.app.social.compose.as_mut().unwrap().poll.as_mut().unwrap().options[2] = "x".repeat(11);
    assert!(compose(&w).problem(Network::Mastodon, &info).is_some_and(|p| p.contains("longer than 10 characters")));
    w.click("compose:poll-remove:2");
    // the duration starts at one day (inside the server's limits) and the switches work
    assert_eq!(compose(&w).poll.as_ref().unwrap().expires_in, 86_400);
    w.click("compose:poll-multiple");
    w.click("compose:poll-hide");
    send_it(&mut w);
    wait_for(&mut w, |w| w.app.social.compose.is_none());
    let sent = w.script.sends.lock().unwrap()[0].clone();
    assert_eq!(sent.poll, Some(DraftPoll { options: vec!["Tea".into(), "Coffee".into()], expires_in: 86_400, multiple: true, hide_totals: true }));
    assert_eq!(sent.text, "Tea or coffee?");
}

// spec: SOC-061
#[test]
fn soc_061_the_durations_stay_inside_what_the_server_allows_and_the_defaults_apply_without_an_answer() {
    let l = PollLimits { max_options: 4, max_chars: 50, min_expiry: 3600, max_expiry: 86_400 };
    let all: Vec<u32> = super::poll::ComposePoll::durations(&l).iter().map(|d| d.0).collect();
    assert_eq!(all, [3600, 21_600, 86_400]);
    let mut p = super::poll::ComposePoll { expires_in: 300, ..Default::default() };
    p.fit(&l);
    assert_eq!(p.expires_in, 3600, "the nearest allowed one");
    assert_eq!(PollLimits::default(), PollLimits { max_options: 4, max_chars: 50, min_expiry: 300, max_expiry: 2_592_000 });
    let v = serde_json::json!({"configuration": {"polls": {"max_options": 6, "max_characters_per_option": 80, "min_expiration": 60, "max_expiration": 604_800}}});
    assert_eq!(PollLimits::from_instance(&v), PollLimits { max_options: 6, max_chars: 80, min_expiry: 60, max_expiry: 604_800 });
    assert_eq!(PollLimits::from_instance(&serde_json::json!({})), PollLimits::default());
    // a poll with a tiny minimum still offers a duration
    let tiny = PollLimits { min_expiry: 100, max_expiry: 200, ..PollLimits::default() };
    assert_eq!(super::poll::ComposePoll::durations(&tiny).len(), 1);
}

// spec: SOC-061
#[test]
fn soc_061_a_poll_and_pictures_or_a_video_exclude_each_other_and_the_box_says_so() {
    let mut w = W::new("poll-media");
    let pic = w.dir.join("p.png");
    std::fs::write(&pic, crate::social::actions::tests::png(20, 20, false)).unwrap();
    open_box(&mut w, limits());
    // a picture first: the poll is not offered, and the note says why
    w.app.social.compose.as_mut().unwrap().path = pic.to_string_lossy().into_owned();
    w.frames(2);
    w.click("compose:add-picture");
    assert_eq!(compose(&w).pictures.len(), 1);
    w.rect("compose:poll-note");
    w.click("compose:add-poll");
    assert!(compose(&w).poll.is_none(), "no poll beside a picture");
    w.click("compose:remove:0");
    // a poll first: no picture control, and a file that arrives anyway is refused
    w.click("compose:add-poll");
    assert!(compose(&w).poll.is_some());
    assert!(!w.app.social.rects.iter().any(|r| r.0 == "compose:choose" || r.0 == "compose:add-picture"));
    let info = limits();
    let e = w.app.social.compose.as_mut().unwrap().attach_files(&[pic], &info).unwrap_err();
    assert!(e.contains("A poll and pictures or a video cannot be in one post"), "{e}");
    assert!(compose(&w).pictures.is_empty());
    // removing the poll brings the picture controls back
    w.click("compose:poll-remove");
    w.rect("compose:choose");
}

// spec: SOC-061, SOC-063
#[test]
fn soc_061_bluesky_has_no_polls_and_no_scheduling() {
    let mut w = W::new("bsky-extras");
    w.app.cfg.social_accounts[0].account.network = Network::Bluesky;
    w.click("new-post");
    w.rect("compose:language");
    for k in ["compose:add-poll", "compose:schedule", "compose:scheduled-list"] {
        assert!(!w.app.social.rects.iter().any(|r| r.0 == k), "{k} is not offered on Bluesky");
    }
}

// spec: SOC-062
#[test]
fn soc_062_a_poll_on_a_card_shows_options_to_vote_for_and_the_bars_after_voting_expiry_or_for_its_author() {
    let mut w = W::new("poll-card");
    with_poll(&mut w, poll(false, false));
    // not voted: the options are for choosing, no percentages
    for i in 0..3 {
        w.rect(&format!("poll:0:option:{i}"));
    }
    w.rect("poll:0:vote");
    // after voting: bars, the vote control is gone
    with_poll(&mut w, poll(true, false));
    assert!(!w.app.social.rects.iter().any(|r| r.0 == "poll:0:vote"));
    // closed: the same
    with_poll(&mut w, poll(false, true));
    assert!(!w.app.social.rects.iter().any(|r| r.0 == "poll:0:vote"));
    // the poll of the account itself shows its results and has no vote control
    with_poll(&mut w, poll(false, false));
    let key = (home().account, "home".to_string());
    w.app.social.timelines.get_mut(&key).unwrap().posts[0].author.handle = "ada@demo.social".into();
    w.frames(3);
    assert!(!w.app.social.rects.iter().any(|r| r.0 == "poll:0:vote"));
    // the numbers
    let p = poll(true, false);
    assert_eq!((p.percent(0), p.percent(1), p.percent(2)), (Some(75), Some(25), Some(0)));
    let hidden = Poll { options: p.options.iter().map(|o| PollOption { votes: None, ..o.clone() }).collect(), ..p.clone() };
    assert_eq!(hidden.percent(0), None, "totals the author hides show no share");
    let multi = Poll { multiple: true, votes_count: 9, voters_count: Some(4), ..p.clone() };
    assert_eq!(multi.percent(0), Some(75), "several choices count people, not votes");
    assert_eq!(super::poll::duration_words(7200), "2 h");
    assert_eq!(super::poll::duration_words(86_400 * 3), "3 days");
    assert_eq!(p.time_left(crate::calendar::time::now()), "2 h left");
}

// spec: SOC-062
#[test]
fn soc_062_voting_needs_a_choice_shows_a_spinner_in_the_button_and_the_answer_replaces_the_poll() {
    let mut w = W::new("poll-vote");
    with_poll(&mut w, poll(false, false));
    // nothing chosen: the button does nothing
    w.click("poll:0:vote");
    assert!(w.script.votes.lock().unwrap().is_empty());
    let (tx, rx) = std::sync::mpsc::channel();
    *w.script.vote_gate.lock().unwrap() = Some(rx);
    w.click("poll:0:option:1");
    let before = w.rect("poll:0:vote");
    w.click("poll:0:vote");
    w.frames(2);
    assert!(w.app.social.rects.iter().any(|r| r.0 == "poll:0:vote-spinner"), "a spinner in the button");
    assert_eq!(w.rect("poll:0:vote").size(), before.size(), "the button keeps its size");
    // pressed again while it runs: nothing is sent twice
    w.click("poll:0:vote");
    assert_eq!(w.script.votes.lock().unwrap().len(), 1);
    assert!(!first_poll(&w).voted, "nothing changes before the answer");
    tx.send(()).unwrap();
    wait_for(&mut w, |w| first_poll(w).voted);
    w.frames(3);
    assert_eq!((first_poll(&w).own_votes, first_poll(&w).votes_count), (vec![1], 5));
    assert_eq!(*w.script.votes.lock().unwrap(), [("p1".to_string(), vec![1usize])]);
    assert!(!w.app.social.rects.iter().any(|r| r.0 == "poll:0:vote"), "the bars replace the vote control");
}

// spec: SOC-062
#[test]
fn soc_062_a_poll_with_several_choices_sends_all_ticked_and_a_refused_vote_says_why() {
    let mut w = W::new("poll-multi");
    with_poll(&mut w, Poll { multiple: true, ..poll(false, false) });
    w.click("poll:0:option:0");
    w.click("poll:0:option:2");
    w.script.vote_answers.lock().unwrap().push(Err(super::actions::ActionError::Failed("you already voted".into())));
    w.click("poll:0:vote");
    wait_for(&mut w, |w| toast_has(w, "Could not vote: you already voted"));
    assert_eq!(w.script.votes.lock().unwrap()[0].1, vec![0, 2]);
    assert!(!first_poll(&w).voted, "a refused vote changes nothing");
    // the button works again, and the ticks are still there
    w.click("poll:0:vote");
    wait_for(&mut w, |w| first_poll(w).voted);
    assert_eq!(first_poll(&w).own_votes, vec![0, 2]);
}

// spec: SOC-063
#[test]
fn soc_063_a_post_with_a_time_is_scheduled_the_confirm_names_it_and_nothing_goes_into_the_feed() {
    let mut w = W::new("schedule");
    open_box(&mut w, InstanceInfo::defaults(Network::Mastodon));
    type_text(&mut w, "Later");
    w.click("compose:schedule");
    // an hour ahead, in the local time zone
    let at = compose(&w).schedule.as_ref().unwrap().parse(crate::calendar::time::now()).unwrap();
    let now = crate::calendar::time::now();
    assert!(at > now + 3600 && at <= now + 3600 + 900, "{}", at - now);
    // too soon, and not a time at all: named, and Send waits
    let set = |w: &mut W, d: &str, t: &str| w.app.social.compose.as_mut().unwrap().schedule = Some(ScheduleInput { date: d.into(), time: t.into() });
    let d = ScheduleInput::at(now + 60);
    set(&mut w, &d.date, &d.time);
    w.frames(2);
    let info = w.app.social.info_for(&w.app.cfg.social_accounts[0].account);
    assert_eq!(compose(&w).problem(Network::Mastodon, &info).as_deref(), Some("A scheduled post must be at least 5 minutes ahead."));
    set(&mut w, "tomorrow", "9:00");
    assert!(compose(&w).problem(Network::Mastodon, &info).is_some_and(|p| p.contains("year-month-day")));
    set(&mut w, "2099-01-01", "25:00");
    assert!(compose(&w).problem(Network::Mastodon, &info).is_some_and(|p| p.contains("hours:minutes")));
    // a good time
    let later = ScheduleInput::at(now + 86_400);
    set(&mut w, &later.date, &later.time);
    w.frames(2);
    let want = later.parse(now).unwrap();
    assert!(want >= now + MIN_AHEAD);
    ctrl_enter(&mut w);
    assert_eq!(compose(&w).stage, super::compose::Stage::Confirm);
    w.rect("compose:confirm-schedule");
    w.click("compose:confirm-post");
    wait_for(&mut w, |w| w.app.social.compose.is_none());
    w.frames(3);
    let sent = w.script.sends.lock().unwrap()[0].clone();
    assert_eq!(sent.schedule, Some(want));
    assert!(toast_has(&w, "Scheduled for"), "{:?}", w.app.toasts);
    let key = (home().account, "home".to_string());
    assert!(w.app.social.timelines[&key].waiting.is_empty(), "a scheduled post is no post: nothing waits in the feed");
    assert_eq!(w.script.scheduled.lock().unwrap().len(), 1);
}

// spec: SOC-063
#[test]
fn soc_063_the_time_is_typed_in_local_time_and_goes_out_as_utc() {
    let now = crate::calendar::time::now();
    let target = now + 2 * 86_400;
    let input = ScheduleInput::at(target);
    let got = input.parse(now).unwrap();
    assert!(got <= target && target - got < 900, "rounded down to a quarter of an hour: {}", target - got);
    assert_eq!(super::actions::iso_at(1_700_000_000), "2023-11-14T22:13:20.000Z");
    let s = Scheduled::parse(&serde_json::json!({"id": "7", "scheduled_at": "2030-01-01T10:00:00.000Z", "params": {"text": "t", "visibility": "unlisted", "poll": null}, "media_attachments": [{}]})).unwrap();
    assert_eq!((s.id.as_str(), s.at, s.text.as_str(), s.visibility.as_str(), s.media, s.poll), ("7", 1_893_492_000, "t", "unlisted", 1, false));
    assert!(Scheduled::parse(&serde_json::json!({"id": "a b", "scheduled_at": "2030-01-01T10:00:00Z"})).is_none());
    assert!(Scheduled::parse(&serde_json::json!({"id": "1"})).is_none());
}

// spec: SOC-064
#[test]
fn soc_064_the_scheduled_posts_are_listed_from_the_box_and_the_menu_and_a_cancel_deletes_one() {
    let mut w = W::new("scheduled-list");
    let at = crate::calendar::time::now() + 86_400;
    *w.script.scheduled.lock().unwrap() = vec![
        Scheduled { id: "s2".into(), at: at + 600, text: "second".into(), visibility: "public".into(), media: 1, poll: false },
        Scheduled { id: "s1".into(), at, text: "first".into(), visibility: "unlisted".into(), media: 0, poll: true },
    ];
    // from the menu of the feed
    w.click("feed-menu");
    w.click("scheduled-posts");
    wait_for(&mut w, |w| w.app.social.scheduled.items.len() == 2);
    w.frames(3);
    assert_eq!(w.app.social.scheduled.items.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["s1", "s2"], "the earliest first");
    w.rect("scheduled:cancel:1");
    w.click("scheduled:cancel:0");
    wait_for(&mut w, |w| w.app.social.scheduled.items.len() == 1);
    assert_eq!(*w.script.cancelled.lock().unwrap(), ["s1"]);
    assert!(toast_has(&w, "Scheduled post cancelled"));
    w.click("scheduled:refresh");
    wait_for(&mut w, |w| !w.app.social.scheduled.loading);
    assert_eq!(w.app.social.scheduled.items.len(), 1);
    w.click("scheduled:close");
    assert!(w.app.social.scheduled.tab.is_none());
    // from the compose box
    w.click("new-post");
    w.click("compose:scheduled-list");
    wait_for(&mut w, |w| w.app.social.scheduled.items.len() == 1);
    w.click("scheduled:cancel:0");
    wait_for(&mut w, |w| w.app.social.scheduled.items.is_empty());
    w.frames(3);
    w.rect("scheduled:empty");
}

// spec: SOC-065
#[test]
fn soc_065_a_post_starts_with_the_language_of_the_setting_and_the_last_choice_of_the_account_and_sends_it() {
    let mut w = W::new("language");
    w.app.cfg.social_post_language = "de".into();
    w.frames(3);
    w.click("new-post");
    assert_eq!(compose(&w).language, "de", "the setting");
    w.rect("compose:language");
    w.app.social.compose.as_mut().unwrap().language = "fr".into();
    type_text(&mut w, "Bonjour");
    send_it(&mut w);
    wait_for(&mut w, |w| w.app.social.compose.is_none());
    assert_eq!(w.script.sends.lock().unwrap()[0].language, "fr");
    // the choice is kept in the account's entry of the settings (it survives a restart) and is no secret
    w.frames(3);
    assert_eq!(w.app.cfg.social_accounts[0].account.post_language.as_deref(), Some("fr"));
    let json = serde_json::to_string(&w.app.cfg.social_accounts[0]).unwrap();
    assert!(json.contains(r#""post_language":"fr""#), "{json}");
    let back: crate::social::SocialAccountEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(back.account.post_language.as_deref(), Some("fr"));
    let old: crate::social::Account = serde_json::from_str(r#"{"id":"a","network":"mastodon","server":"https://m.test","handle":"a@m.test","display_name":"A"}"#).unwrap();
    assert_eq!(old.post_language, None, "an account from before has none: the setting decides");
    assert!(!serde_json::to_string(&old).unwrap().contains("post_language"));
    // the next post of the account starts with that choice, not with the setting
    w.click("new-post");
    assert_eq!(compose(&w).language, "fr");
    // no language: none is sent
    w.app.social.compose.as_mut().unwrap().language.clear();
    type_text(&mut w, "x");
    send_it(&mut w);
    wait_for(&mut w, |w| w.app.social.compose.is_none());
    assert_eq!(w.script.sends.lock().unwrap()[1].language, "");
    w.frames(3);
    assert_eq!(w.app.cfg.social_accounts[0].account.post_language.as_deref(), Some(""), "No language is a choice too");
    // a setting that is no code is the system's language
    w.app.cfg.social_post_language = "Deutsch".into();
    w.frames(3);
    assert_eq!(w.app.social.post_language, super::language::system());
}

// spec: SOC-065
#[test]
fn soc_065_languages_come_from_the_locale_and_names_are_readable() {
    use super::language::*;
    assert_eq!(from_locale("de_DE.UTF-8").as_deref(), Some("de"));
    assert_eq!(from_locale("en-US").as_deref(), Some("en"));
    assert_eq!(from_locale("pt_BR").as_deref(), Some("pt"));
    assert_eq!((from_locale("C"), from_locale("C.UTF-8"), from_locale(""), from_locale("xx_XX")), (None, None, None, None));
    assert_eq!((name("de"), name(""), name("xx")), ("German (de)".to_string(), "No language".to_string(), "xx".to_string()));
    assert!(is_code("de") && is_code("") && !is_code("DE") && !is_code("deu") && !is_code("d1"));
    assert!(LANGUAGES.iter().all(|(c, n)| is_code(c) && !n.is_empty()));
    assert!(LANGUAGES.windows(2).all(|p| p[0].0 < p[1].0), "sorted by code");
    assert_eq!(crate::config::Config::default().social_post_language, system());
}

fn clip(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    // the start of an MP4 file: a box of 24 bytes of type ftyp, brand isom
    let mut b = vec![0, 0, 0, 24, b'f', b't', b'y', b'p', b'i', b's', b'o', b'm', 0, 0, 2, 0, b'i', b's', b'o', b'm', b'm', b'p', b'4', b'1'];
    b.extend_from_slice(&[0u8; 200]);
    let p = dir.join(name);
    std::fs::write(&p, b).unwrap();
    p
}

// spec: SOC-066
#[test]
fn soc_066_a_video_is_one_per_post_not_mixed_with_pictures_and_gets_its_own_description() {
    let mut w = W::new("video-box");
    let (v1, v2) = (clip(&w.dir, "a.mp4"), clip(&w.dir, "b.mp4"));
    let pic = w.dir.join("p.png");
    std::fs::write(&pic, crate::social::actions::tests::png(20, 20, false)).unwrap();
    open_box(&mut w, InstanceInfo { video_bytes: 1000, ..InstanceInfo::defaults(Network::Mastodon) });
    let info = w.app.social.info_for(&w.app.cfg.social_accounts[0].account);
    // the path field takes a video as it takes a picture
    w.app.social.compose.as_mut().unwrap().path = v1.to_string_lossy().into_owned();
    w.frames(2);
    w.click("compose:add-picture");
    assert_eq!(compose(&w).video.as_ref().map(|v| v.path.clone()), Some(v1.clone()));
    assert_eq!(compose(&w).problem(Network::Mastodon, &info), None, "a video alone is a post");
    // the picture controls are gone; a second video, a picture and a poll are refused with words
    assert!(!w.app.social.rects.iter().any(|r| r.0 == "compose:choose"));
    let c = w.app.social.compose.as_mut().unwrap();
    assert!(c.attach_files(&[v2], &info).unwrap_err().contains("one video"));
    assert!(c.attach_files(std::slice::from_ref(&pic), &info).unwrap_err().contains("pictures or a video, not both"));
    w.click("compose:add-poll");
    assert!(compose(&w).poll.is_none());
    w.click("compose:video-alt");
    w.frame(vec![Event::Text("A cat".into())]);
    w.frames(2);
    assert_eq!(compose(&w).video.as_ref().unwrap().alt, "A cat");
    w.click("compose:video-remove");
    assert!(compose(&w).video.is_none());
    // a picture first: a video is refused
    let c = w.app.social.compose.as_mut().unwrap();
    c.attach_files(&[pic], &info).unwrap();
    assert!(c.attach_files(&[v1], &info).unwrap_err().contains("pictures or a video, not both"));
    // over the limit of the server: refused with both sizes
    let big = w.dir.join("big.mp4");
    let mut bytes = std::fs::read(w.dir.join("a.mp4")).unwrap();
    bytes.resize(5000, 0);
    std::fs::write(&big, bytes).unwrap();
    w.app.social.compose.as_mut().unwrap().pictures.clear();
    let e = w.app.social.compose.as_mut().unwrap().attach_files(&[big], &info).unwrap_err();
    assert!(e.contains("big.mp4") && e.contains("videos can be up to"), "{e}");
}

// spec: SOC-066
#[test]
fn soc_066_a_video_goes_out_with_the_post() {
    let mut w = W::new("video-send");
    let v = clip(&w.dir, "a.mp4");
    open_box(&mut w, InstanceInfo::defaults(Network::Mastodon));
    type_text(&mut w, "Look");
    let info = w.app.social.info_for(&w.app.cfg.social_accounts[0].account);
    w.app.social.compose.as_mut().unwrap().attach_files(std::slice::from_ref(&v), &info).unwrap();
    send_it(&mut w);
    wait_for(&mut w, |w| w.app.social.compose.is_none());
    let sent = w.script.sends.lock().unwrap()[0].clone();
    assert_eq!(sent.video.map(|v| v.path), Some(v));
}

// spec: SOC-063
#[test]
fn soc_063_the_date_is_picked_in_the_calendars_month_grid_and_the_time_is_two_numbers_and_typing_still_works() {
    let mut w = W::new("schedule-pick");
    open_box(&mut w, InstanceInfo::defaults(Network::Mastodon));
    w.click("compose:schedule");
    let now = crate::calendar::time::now();
    let target = ScheduleInput::at(now + 5 * 86_400);
    // the 📅 button opens the month of the date in the field; a click on a day sets the field
    w.click("pick:schedule-day");
    w.click(&format!("pick-day:{}", target.date));
    assert_eq!(compose(&w).schedule.as_ref().unwrap().date, target.date);
    // the hour and the minute: click one, type a number, Enter
    w.click("compose:schedule-hour");
    w.frame(vec![Event::Text("9".into())]);
    w.frame(vec![Event::Key { key: eframe::egui::Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: eframe::egui::Modifiers::NONE }]);
    w.frames(3);
    w.click("compose:schedule-minute");
    w.frame(vec![Event::Text("45".into())]);
    w.frame(vec![Event::Key { key: eframe::egui::Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: eframe::egui::Modifiers::NONE }]);
    w.frames(3);
    assert_eq!(compose(&w).schedule.as_ref().unwrap().time, "09:45");
    let got = compose(&w).schedule.as_ref().unwrap().parse(now).unwrap();
    assert_eq!(crate::calendar::time::utc_to_local(got).1, 9 * 3600 + 45 * 60);
    // typing the date and a time like "9" or "0930" stays possible
    let typed = |d: &str, t: &str| ScheduleInput { date: d.into(), time: t.into() }.parse(now).unwrap();
    assert_eq!(typed("2099-01-01", "0930"), typed("2099-01-01", "9:30"));
    assert_eq!(typed("2099-01-01", "9"), typed("2099-01-01", "09:00"));
}
