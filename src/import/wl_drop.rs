//! Drag and drop from other programs on Wayland. The window toolkit only takes dropped files
//! on X11, so on Wayland Shale listens for drops itself, on the window's own connection (the
//! way the clipboard helper does). It accepts file lists, whole e-mail messages, calendar data
//! and plain text, and only ever asks for a *copy* (so a mail program never moves a message
//! out of its folder because of a drop).
//!
//! Compositors pick different listeners when one program has several: some the first one,
//! some the newest, some all of them. Shale therefore listens twice (before and after the
//! clipboard helper starts) and ignores the second copy of the same drop.

use std::collections::HashMap;
use std::io::Read;
use std::os::fd::{BorrowedFd, FromRawFd};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use wayland_client::backend::{Backend, ObjectId};
use wayland_client::protocol::{wl_data_device, wl_data_device_manager, wl_data_offer, wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};

/// What happened with a drag from another program. Positions are in window pixels.
#[derive(Debug, Clone)]
pub enum DropEvent {
    Hover {
        x: f32,
        y: f32,
    },
    Leave,
    /// `parts`: every useful kind the other program offered, with what it sent for it;
    /// `report`: for troubleshooting, how each kind went
    Drop {
        x: f32,
        y: f32,
        parts: Vec<(String, Vec<u8>)>,
        offered: Vec<String>,
        report: Vec<String>,
    },
    /// a drop is being read
    Busy,
    /// something was dragged over the window that Shale can't take
    /// (sent while it moves over the window, so the window can say so at the pointer)
    Refused {
        x: f32,
        y: f32,
        offered: Vec<String>,
    },
}

/// The kinds Shale takes, best first.
pub const ACCEPTED: [&str; 15] =
    ["text/uri-list", "message/rfc822", "text/calendar", "text/x-moz-url", "text/plain;charset=utf-8", "UTF8_STRING", "text/plain", "image/png", "image/jpeg", "image/webp", "image/gif", "image/bmp", "video/mp4", "video/webm", "video/ogg"];

/// Kinds read on a drop (pictures from a web browser, IMP-032; a mail program may send an empty file list but a link to the
/// message and its file name).
const WANTED: [&str; 18] = [
    "text/uri-list",
    "message/rfc822",
    "text/calendar",
    "text/x-moz-url",
    "application/x-moz-file-promise-url",
    "application/x-moz-file-promise-dest-filename",
    "text/plain;charset=utf-8",
    "utf8_string",
    "text/plain",
    "string",
    "image/png",
    "image/jpeg",
    "image/webp",
    "image/gif",
    "image/bmp",
    "video/mp4",
    "video/webm",
    "video/ogg",
];

/// "text/plain; charset=UTF-8" → "text/plain;charset=utf-8"
pub fn norm(m: &str) -> String {
    m.to_ascii_lowercase().replace(' ', "")
}

/// The best kind among the offered ones.
pub fn pick_mime(offered: &[String]) -> Option<String> {
    ACCEPTED.iter().find_map(|a| offered.iter().find(|o| norm(o) == norm(a)).cloned())
}

/// The offered kinds worth reading, best first, each once (with the spelling the program used).
pub fn wanted_kinds(offered: &[String]) -> Vec<String> {
    WANTED.iter().filter_map(|w| offered.iter().find(|o| norm(o) == *w).cloned()).collect()
}

/// The offered kinds without repeats (some programs list everything twice).
pub fn unique(offered: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for o in offered {
        if !out.iter().any(|x| norm(x) == norm(o)) {
            out.push(o.clone());
        }
    }
    out
}

/// What came for one kind.
#[derive(Debug, Clone, Default)]
pub struct Got {
    pub data: Vec<u8>,
    /// the other program closed the transfer (otherwise it did not answer in time)
    pub closed: bool,
}

/// Everything read for the offered kinds: each kind with what came for it.
type KindData = Vec<(String, Got)>;

/// Kinds read while the drag is still over the window (some bridges, like the one for X11
/// programs, end the transfer soon after the drop). Not the file list: a mail program may
/// write the promised file for it, which should only happen on a drop.
fn early(kind: &str) -> bool {
    // the file list too: a Wayland mail program may write the message file for it
    !kind.is_empty()
}

/// Ask for several kinds at once and read them all (up to `timeout`).
fn read_kinds(offer: &wl_data_offer::WlDataOffer, conn: &Connection, kinds: &[String], timeout: Duration) -> Vec<(String, Got)> {
    let mut out: Vec<(String, Got)> = kinds.iter().map(|k| (k.clone(), Got::default())).collect();
    let mut open: Vec<(usize, std::fs::File, i32)> = Vec::new();
    for (i, k) in kinds.iter().enumerate() {
        let mut fds = [0i32; 2];
        if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC | libc::O_NONBLOCK) } != 0 {
            continue;
        }
        offer.receive(k.clone(), unsafe { BorrowedFd::borrow_raw(fds[1]) });
        let _ = conn.flush();
        unsafe { libc::close(fds[1]) };
        open.push((i, unsafe { std::fs::File::from_raw_fd(fds[0]) }, fds[0]));
    }
    for (i, g) in drain(open, kinds.len(), timeout).into_iter().enumerate() {
        out[i].1 = g;
    }
    out
}

/// Read from several pipes at once until each is closed or the time is up.
fn drain(mut open: Vec<(usize, std::fs::File, i32)>, n: usize, timeout: Duration) -> Vec<Got> {
    let mut out: Vec<((), Got)> = (0..n).map(|_| ((), Got::default())).collect();
    let started = Instant::now();
    let mut buf = vec![0u8; 64 * 1024];
    while !open.is_empty() {
        let left = timeout.as_millis() as i64 - started.elapsed().as_millis() as i64;
        if left <= 0 {
            break;
        }
        let mut polls: Vec<libc::pollfd> = open.iter().map(|(_, _, fd)| libc::pollfd { fd: *fd, events: libc::POLLIN, revents: 0 }).collect();
        if unsafe { libc::poll(polls.as_mut_ptr(), polls.len() as libc::nfds_t, left as i32) } <= 0 {
            break;
        }
        let mut done = Vec::new();
        for (j, p) in polls.iter().enumerate() {
            if p.revents == 0 {
                continue;
            }
            let (i, f, _) = &mut open[j];
            loop {
                match f.read(&mut buf) {
                    Ok(0) => {
                        out[*i].1.closed = true;
                        done.push(j);
                        break;
                    }
                    Ok(n) => {
                        out[*i].1.data.extend_from_slice(&buf[..n]);
                        if out[*i].1.data.len() > 512 << 20 {
                            done.push(j);
                            break;
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(_) => {
                        out[*i].1.closed = true;
                        done.push(j);
                        break;
                    }
                }
            }
        }
        for j in done.into_iter().rev() {
            open.remove(j);
        }
    }
    out.into_iter().map(|(_, g)| g).collect()
}

/// One line per kind for the troubleshooting list.
fn report_line(kind: &str, early: Option<&Got>, late: Option<&Got>) -> String {
    let say = |g: Option<&Got>| match g {
        None => "not asked".to_string(),
        Some(g) if !g.data.is_empty() => format!("{} bytes", g.data.len()),
        Some(g) if g.closed => "empty".to_string(),
        Some(_) => "no answer".to_string(),
    };
    format!("  {kind}: while dragging {}, after the drop {}", say(early), say(late))
}

/// A drag from another program over the window, without any protocol: what the data offer
/// listed, whether Shale takes it, and where the pointer is (IMP-012). The Wayland glue only
/// feeds it events and sends what it returns; tests feed it the same way.
#[derive(Debug, Clone)]
pub struct Drag {
    offered: Vec<String>,
    /// the kind accepted (None: Shale can't take this drag)
    mime: Option<String>,
    pos: (f64, f64),
}

impl Drag {
    /// The drag enters the window at `x`,`y`: the drag and the event to show.
    pub fn enter(offered: Vec<String>, x: f64, y: f64) -> (Drag, DropEvent) {
        let d = Drag { mime: pick_mime(&offered), offered, pos: (x, y) };
        let e = d.over();
        (d, e)
    }

    /// The kind accepted, if any.
    pub fn mime(&self) -> Option<&str> {
        self.mime.as_deref()
    }

    /// The pointer moves.
    pub fn motion(&mut self, x: f64, y: f64) -> DropEvent {
        self.pos = (x, y);
        self.over()
    }

    fn over(&self) -> DropEvent {
        let (x, y) = (self.pos.0 as f32, self.pos.1 as f32);
        match &self.mime {
            Some(_) => DropEvent::Hover { x, y },
            None => DropEvent::Refused { x, y, offered: unique(&self.offered) },
        }
    }

    /// The kinds to read while the drag is still over the window (none for a refused drag).
    pub fn prefetch_kinds(&self) -> Vec<String> {
        if self.mime.is_none() {
            return vec![];
        }
        wanted_kinds(&self.offered).into_iter().filter(|k| early(k)).collect()
    }

    /// The drop: what to read, or (for a drag Shale can't take) the event that ends it.
    pub fn drop(self) -> Result<Landing, DropEvent> {
        if self.mime.is_none() {
            return Err(DropEvent::Leave);
        }
        Ok(Landing { x: self.pos.0 as f32, y: self.pos.1 as f32, offered: self.offered })
    }
}

/// A drop whose data is to be read.
#[derive(Debug, Clone)]
pub struct Landing {
    x: f32,
    y: f32,
    offered: Vec<String>,
}

impl Landing {
    /// The kinds to ask for after the drop, in reading order (the quick text kinds first).
    pub fn read_order(&self) -> Vec<String> {
        let kinds = wanted_kinds(&self.offered);
        let mut order: Vec<String> = kinds.iter().filter(|k| early(k)).cloned().collect();
        order.extend(kinds.iter().filter(|k| !early(k)).cloned());
        order
    }

    /// The finished drop: `before` was read while dragging, `late` after the drop; the data of
    /// the drop wins, the earlier data is the fallback (IMP-013).
    pub fn finish(self, before: &[(String, Got)], late: &[(String, Got)]) -> DropEvent {
        let find = |v: &[(String, Got)], k: &str| v.iter().find(|(m, _)| m == k).map(|(_, g)| g.clone());
        let mut parts = Vec::new();
        let mut report = Vec::new();
        for k in wanted_kinds(&self.offered) {
            let (e, l) = (find(before, &k), find(late, &k));
            report.push(report_line(&k, e.as_ref(), l.as_ref()));
            let data = l.as_ref().filter(|g| !g.data.is_empty()).or(e.as_ref().filter(|g| !g.data.is_empty())).map(|g| g.data.clone()).unwrap_or_default();
            parts.push((k, data));
        }
        DropEvent::Drop { x: self.x, y: self.y, parts, offered: unique(&self.offered), report }
    }
}

/// The same drop reaching the second listener (IMP-013): `last` is when the previous drop was
/// taken; true when this one came too soon after it. Otherwise `now` is kept as `last`.
fn is_repeat(last: &mut Option<Instant>, now: Instant) -> bool {
    if last.map(|t| now.saturating_duration_since(t) < Duration::from_millis(800)).unwrap_or(false) {
        return true;
    }
    *last = Some(now);
    false
}

struct Shared {
    tx: Sender<DropEvent>,
    rx: Mutex<Option<Receiver<DropEvent>>>,
    ctx: Mutex<Option<eframe::egui::Context>>,
    last_drop: Mutex<Option<Instant>>,
}

fn shared() -> &'static Shared {
    static S: OnceLock<Shared> = OnceLock::new();
    S.get_or_init(|| {
        let (tx, rx) = channel();
        Shared { tx, rx: Mutex::new(Some(rx)), ctx: Mutex::new(None), last_drop: Mutex::new(None) }
    })
}

fn send(e: DropEvent) {
    let s = shared();
    let _ = s.tx.send(e);
    if let Some(c) = s.ctx.lock().unwrap().as_ref() {
        c.request_repaint();
    }
}

/// Lets a drop wake the window up.
pub fn set_context(ctx: &eframe::egui::Context) {
    *shared().ctx.lock().unwrap() = Some(ctx.clone());
}

/// Drop events that arrived since the last call.
pub fn take_events() -> Vec<DropEvent> {
    let s = shared();
    let rx = s.rx.lock().unwrap();
    rx.as_ref().map(|r| r.try_iter().collect()).unwrap_or_default()
}

/// Start listening on a window's Wayland connection. Does nothing (false) for other displays.
pub fn attach(handle: &impl winit::raw_window_handle::HasDisplayHandle) -> bool {
    let Ok(h) = handle.display_handle() else { return false };
    let winit::raw_window_handle::RawDisplayHandle::Wayland(w) = h.as_raw() else { return false };
    let display = w.display.as_ptr();
    // SAFETY: the display belongs to the window's event loop, which lives as long as the program.
    let backend = unsafe { Backend::from_foreign_display(display.cast()) };
    let conn = Connection::from_backend(backend);
    std::thread::Builder::new()
        .name("shale-drop".into())
        .spawn(move || {
            let mut queue = conn.new_event_queue::<State>();
            let qh = queue.handle();
            conn.display().get_registry(&qh, ());
            let mut state = State { conn: conn.clone(), manager: None, seats: vec![], devices: vec![], offers: HashMap::new(), current: None, prefetch: None };
            while queue.blocking_dispatch(&mut state).is_ok() {}
        })
        .is_ok()
}

struct State {
    conn: Connection,
    manager: Option<wl_data_device_manager::WlDataDeviceManager>,
    seats: Vec<wl_seat::WlSeat>,
    devices: Vec<wl_data_device::WlDataDevice>,
    /// kinds offered by each pending offer
    offers: HashMap<ObjectId, Vec<String>>,
    /// the drag over the window: its offer and what Shale made of it
    current: Option<(wl_data_offer::WlDataOffer, Drag)>,
    /// data read while dragging, for the current offer
    prefetch: Option<(ObjectId, std::thread::JoinHandle<KindData>)>,
}

impl State {
    fn add_devices(&mut self, qh: &QueueHandle<Self>) {
        let Some(m) = &self.manager else { return };
        while self.devices.len() < self.seats.len() {
            let seat = &self.seats[self.devices.len()];
            self.devices.push(m.get_data_device(seat, qh, ()));
            if std::env::var_os("SHALE_DEBUG").is_some() {
                eprintln!("wl_drop: listening for drops (seat {})", self.devices.len());
            }
        }
    }
}

impl Dispatch<wl_registry::WlRegistry, ()> for State {
    fn event(state: &mut Self, reg: &wl_registry::WlRegistry, event: wl_registry::Event, _: &(), _: &Connection, qh: &QueueHandle<Self>) {
        if let wl_registry::Event::Global { name, interface, version } = event {
            match interface.as_str() {
                "wl_seat" => {
                    state.seats.push(reg.bind(name, version.min(5), qh, ()));
                    state.add_devices(qh);
                }
                "wl_data_device_manager" => {
                    state.manager = Some(reg.bind(name, version.min(3), qh, ()));
                    state.add_devices(qh);
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for State {
    fn event(_: &mut Self, _: &wl_seat::WlSeat, _: wl_seat::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<wl_data_device_manager::WlDataDeviceManager, ()> for State {
    fn event(_: &mut Self, _: &wl_data_device_manager::WlDataDeviceManager, _: wl_data_device_manager::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<wl_data_offer::WlDataOffer, ()> for State {
    fn event(state: &mut Self, offer: &wl_data_offer::WlDataOffer, event: wl_data_offer::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let wl_data_offer::Event::Offer { mime_type } = event {
            state.offers.entry(offer.id()).or_default().push(mime_type);
        }
    }
}

impl Dispatch<wl_data_device::WlDataDevice, ()> for State {
    fn event(state: &mut Self, _: &wl_data_device::WlDataDevice, event: wl_data_device::Event, _: &(), conn: &Connection, _: &QueueHandle<Self>) {
        use wl_data_device::Event as E;
        use wl_data_device_manager::DndAction;
        match event {
            E::DataOffer { id } => {
                state.offers.entry(id.id()).or_default();
            }
            E::Enter { serial, x, y, id, .. } => {
                let Some(offer) = id else { return };
                let (drag, event) = Drag::enter(state.offers.remove(&offer.id()).unwrap_or_default(), x, y);
                offer.accept(serial, drag.mime().map(str::to_string));
                if offer.version() >= 3 {
                    // a copy only: never let the other program move (delete) its original
                    offer.set_actions(DndAction::Copy, DndAction::Copy);
                }
                send(event);
                // read what can be read already now (the drop may come too late for some programs)
                let kinds = drag.prefetch_kinds();
                if !kinds.is_empty() {
                    let (o2, c2) = (offer.clone(), state.conn.clone());
                    // up to 6 s: making the message file may need the mail server
                    let h = std::thread::spawn(move || read_kinds(&o2, &c2, &kinds, Duration::from_millis(6000)));
                    state.prefetch = Some((offer.id(), h));
                }
                state.current = Some((offer, drag));
                let _ = conn.flush();
            }
            E::Motion { x, y, .. } => {
                if let Some((_, drag)) = &mut state.current {
                    send(drag.motion(x, y));
                }
            }
            E::Leave => {
                if let Some((offer, _)) = state.current.take() {
                    offer.destroy();
                }
                send(DropEvent::Leave);
            }
            E::Drop => {
                let Some((offer, drag)) = state.current.take() else { return };
                let landing = match drag.drop() {
                    Ok(l) => l,
                    Err(end) => {
                        offer.destroy();
                        send(end);
                        return;
                    }
                };
                if is_repeat(&mut shared().last_drop.lock().unwrap(), Instant::now()) {
                    offer.destroy();
                    return;
                }
                let conn2 = state.conn.clone();
                let _ = conn.flush();
                let prefetch = state.prefetch.take().filter(|(id, _)| *id == offer.id()).map(|(_, h)| h);
                send(DropEvent::Busy);
                std::thread::spawn(move || {
                    // everything at once, the quick text kinds first, with a short time limit
                    let late = read_kinds(&offer, &conn2, &landing.read_order(), Duration::from_millis(2500));
                    let before: Vec<(String, Got)> = prefetch.and_then(|h| h.join().ok()).unwrap_or_default();
                    if offer.version() >= 3 {
                        offer.finish();
                    }
                    offer.destroy();
                    let _ = conn2.flush();
                    send(landing.finish(&before, &late));
                });
            }
            E::Selection { id: Some(o) } => {
                // the clipboard: handled by egui's clipboard helper
                state.offers.remove(&o.id());
                o.destroy();
            }
            _ => {}
        }
    }

    wayland_client::event_created_child!(State, wl_data_device::WlDataDevice, [
        wl_data_device::EVT_DATA_OFFER_OPCODE => (wl_data_offer::WlDataOffer, ()),
    ]);
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec: IMP-009
    #[test]
    fn picks_the_most_useful_kind() {
        let o = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(pick_mime(&o(&["text/plain", "text/uri-list", "text/x-moz-url"])).as_deref(), Some("text/uri-list"));
        assert_eq!(pick_mime(&o(&["text/plain", "text/calendar"])).as_deref(), Some("text/calendar"));
        assert_eq!(pick_mime(&o(&["application/x-moz-file-promise", "text/x-moz-message"])), None);
        // what Thunderbird offers (each kind twice, spelled in different ways)
        let tb = o(&[
            "text/plain",
            "text/plain; charset=utf-8",
            "UTF8_STRING",
            "STRING",
            "application/x-moz-custom-clipdata",
            "text/x-moz-url",
            "application/x-moz-file-promise-url",
            "text/uri-list",
            "application/x-moz-file-promise",
            "application/x-moz-file-promise-dest-filename",
            "text/plain",
            "text/plain;charset=utf-8",
            "DELETE",
        ]);
        assert_eq!(pick_mime(&tb).as_deref(), Some("text/uri-list"));
        assert_eq!(wanted_kinds(&tb), o(&["text/uri-list", "text/x-moz-url", "application/x-moz-file-promise-url", "application/x-moz-file-promise-dest-filename", "text/plain; charset=utf-8", "UTF8_STRING", "text/plain", "STRING"]));
        assert_eq!(unique(&tb).len(), 11);
        // a browser dragging a picture (IMP-032): the picture kinds are accepted and read
        let img = o(&["text/plain", "image/png", "image/jpeg", "application/x-moz-file-promise-url"]);
        assert_eq!(pick_mime(&img).as_deref(), Some("text/plain"));
        assert_eq!(pick_mime(&o(&["image/webp"])).as_deref(), Some("image/webp"), "a drag of only a picture is taken");
        assert_eq!(pick_mime(&o(&["video/webm"])).as_deref(), Some("video/webm"));
        assert_eq!(wanted_kinds(&img), o(&["application/x-moz-file-promise-url", "text/plain", "image/png", "image/jpeg"]));
        assert!(early("text/uri-list") && early("text/x-moz-url"), "everything is asked for while dragging");
    }

    // spec: IMP-009
    #[test]
    fn reads_several_transfers_at_once_with_a_time_limit() {
        let pipe = || {
            let mut fds = [0i32; 2];
            assert_eq!(unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC | libc::O_NONBLOCK) }, 0);
            (unsafe { std::fs::File::from_raw_fd(fds[0]) }, fds[0], unsafe { std::fs::File::from_raw_fd(fds[1]) })
        };
        let (r0, f0, mut w0) = pipe(); // answers, in two pieces
        let (r1, f1, w1) = pipe(); // closes without data
        let (r2, f2, _w2) = pipe(); // never answers (kept open)
        let t = std::thread::spawn(move || {
            use std::io::Write;
            w0.write_all(b"file:///tmp/a.eml").unwrap();
            std::thread::sleep(Duration::from_millis(50));
            w0.write_all(b"\r\n").unwrap();
            drop(w0);
            drop(w1);
        });
        let started = Instant::now();
        let got = drain(vec![(0, r0, f0), (1, r1, f1), (2, r2, f2)], 3, Duration::from_millis(400));
        t.join().unwrap();
        assert!(started.elapsed() < Duration::from_millis(1500), "gives up on the silent one");
        assert_eq!((got[0].data.as_slice(), got[0].closed), (&b"file:///tmp/a.eml\r\n"[..], true));
        assert_eq!((got[1].data.len(), got[1].closed), (0, true));
        assert_eq!((got[2].data.len(), got[2].closed), (0, false));
        assert!(report_line("text/plain", Some(&got[2]), Some(&got[1])).contains("while dragging no answer, after the drop empty"));
    }

    fn o(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    // spec: IMP-012
    #[test]
    fn imp_012_an_entering_drag_is_hovered_or_refused_and_follows_the_pointer() {
        let (mut d, e) = Drag::enter(o(&["text/plain", "text/uri-list", "text/plain"]), 10.0, 20.0);
        assert!(matches!(e, DropEvent::Hover { x, y } if (x, y) == (10.0, 20.0)));
        assert_eq!(d.mime(), Some("text/uri-list"));
        assert_eq!(d.prefetch_kinds(), o(&["text/uri-list", "text/plain"]));
        assert!(matches!(d.motion(30.5, 40.0), DropEvent::Hover { x, y } if (x, y) == (30.5, 40.0)));
        // a browser tab: refused, with every kind once, wherever it goes
        let (mut d, e) = Drag::enter(o(&["application/x-tab", "DELETE", "DELETE"]), 1.0, 2.0);
        assert!(matches!(&e, DropEvent::Refused { offered, .. } if *offered == o(&["application/x-tab", "DELETE"])));
        assert!(d.mime().is_none() && d.prefetch_kinds().is_empty());
        assert!(matches!(d.motion(5.0, 6.0), DropEvent::Refused { x, y, .. } if (x, y) == (5.0, 6.0)));
    }

    // spec: IMP-012
    #[test]
    fn imp_012_dropping_a_refused_drag_only_ends_it() {
        let (d, _) = Drag::enter(o(&["application/x-tab"]), 1.0, 2.0);
        assert!(matches!(d.drop(), Err(DropEvent::Leave)));
        let (mut d, _) = Drag::enter(o(&["text/uri-list", "text/plain"]), 1.0, 2.0);
        d.motion(7.0, 8.0);
        let l = d.drop().unwrap();
        assert_eq!(l.read_order(), o(&["text/uri-list", "text/plain"]));
        assert!(matches!(l.finish(&[], &[]), DropEvent::Drop { x, y, .. } if (x, y) == (7.0, 8.0)));
    }

    // spec: IMP-013
    #[test]
    fn imp_013_a_finished_drop_prefers_late_data_and_reports_every_kind() {
        let got = |d: &str, closed| Got { data: d.as_bytes().to_vec(), closed };
        let (d, _) = Drag::enter(o(&["text/uri-list", "text/plain; charset=utf-8", "text/x-moz-url", "text/uri-list"]), 3.0, 4.0);
        let l = d.drop().unwrap();
        let before = vec![("text/uri-list".to_string(), got("file:///early", true)), ("text/plain; charset=utf-8".to_string(), got("early text", true)), ("text/x-moz-url".to_string(), got("", false))];
        let late = vec![("text/uri-list".to_string(), got("file:///late", true)), ("text/plain; charset=utf-8".to_string(), got("", true)), ("text/x-moz-url".to_string(), got("", false))];
        let DropEvent::Drop { x, y, parts, offered, report } = l.finish(&before, &late) else { panic!("not a drop") };
        assert_eq!((x, y), (3.0, 4.0));
        assert_eq!(offered, o(&["text/uri-list", "text/plain; charset=utf-8", "text/x-moz-url"]));
        let data: Vec<(&str, &[u8])> = parts.iter().map(|(k, d)| (k.as_str(), d.as_slice())).collect();
        assert_eq!(data, vec![("text/uri-list", &b"file:///late"[..]), ("text/x-moz-url", &b""[..]), ("text/plain; charset=utf-8", &b"early text"[..])]);
        assert_eq!(report.len(), 3);
        assert!(report[0].contains("while dragging 13 bytes, after the drop 12 bytes"), "{report:?}");
        assert!(report[1].contains("while dragging no answer, after the drop no answer"), "{report:?}");
        assert!(report[2].contains("while dragging 10 bytes, after the drop empty"), "{report:?}");
    }

    // spec: IMP-013
    #[test]
    fn imp_013_the_second_copy_of_a_drop_is_ignored() {
        let t0 = Instant::now();
        let mut last = None;
        assert!(!is_repeat(&mut last, t0));
        assert!(is_repeat(&mut last, t0 + Duration::from_millis(300)));
        assert!(is_repeat(&mut last, t0 + Duration::from_millis(799)));
        assert!(!is_repeat(&mut last, t0 + Duration::from_millis(900)), "a new drop later is taken");
        assert!(is_repeat(&mut last, t0 + Duration::from_millis(1000)), "and starts a new window");
    }
}
