//! A pretend Wayland compositor for the tests of the drop glue (DEV-022): it runs in a thread of
//! the test, talks to the glue over a socket pair, offers drags on command and records what the
//! glue answered, so no real compositor is needed.

use super::{listen, DropEvent, Limits, Sink};
use std::collections::HashMap;
use std::io::Write;
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::{wl_compositor as c_compositor, wl_registry as c_registry, wl_surface as c_surface};
use wayland_client::{Connection, Dispatch as ClientDispatch, QueueHandle};
use wayland_server::protocol::wl_data_device::{self, WlDataDevice};
use wayland_server::protocol::wl_data_device_manager::{self, DndAction, WlDataDeviceManager};
use wayland_server::protocol::wl_data_offer::{self, WlDataOffer};
use wayland_server::protocol::wl_data_source::{self, WlDataSource};
use wayland_server::protocol::wl_seat::{self, WlSeat};
use wayland_server::protocol::{wl_compositor, wl_surface};
use wayland_server::{Client, DataInit, Dispatch, Display, DisplayHandle, GlobalDispatch, New, Resource};

/// How long a test waits for anything before it fails (a bug must never hang the suite).
const WAIT: Duration = Duration::from_secs(5);

/// What the glue did, as the compositor saw it.
#[derive(Debug, Clone, PartialEq)]
pub enum Seen {
    /// the glue asked for a data device
    Device,
    Accept(Option<String>),
    SetActions {
        dnd: DndAction,
        preferred: DndAction,
    },
    Receive(String),
    Finish,
    Destroy,
}

/// What the compositor writes when the glue asks for a kind: bytes, or nothing at all
/// (the pipe stays open and silent).
#[derive(Clone)]
pub enum Answer {
    Data(Vec<u8>),
    Silent,
}

enum Cmd {
    Enter { mimes: Vec<String>, x: f64, y: f64 },
    Motion { x: f64, y: f64 },
    Leave,
    Drop,
}

type Log = Arc<Mutex<Vec<Seen>>>;

/// The compositor's own state, handed to every request.
struct Fake {
    log: Log,
    answers: Arc<Mutex<HashMap<String, Answer>>>,
    devices: Vec<WlDataDevice>,
    surface: Option<wl_surface::WlSurface>,
    /// pipes kept open for silent answers
    held: Vec<OwnedFd>,
    serial: u32,
}

impl Fake {
    fn note(&self, s: Seen) {
        self.log.lock().unwrap().push(s);
    }

    fn apply(&mut self, dh: &DisplayHandle, cmd: Cmd) {
        let device = self.devices.first().expect("the glue asked for a data device").clone();
        match cmd {
            Cmd::Enter { mimes, x, y } => {
                let client = device.client().expect("client of the device");
                let offer: WlDataOffer = client.create_resource::<WlDataOffer, (), Fake>(dh, device.version(), ()).unwrap();
                device.data_offer(&offer);
                for m in mimes {
                    offer.offer(m);
                }
                self.serial += 1;
                device.enter(self.serial, self.surface.as_ref().expect("a surface"), x, y, Some(&offer));
            }
            Cmd::Motion { x, y } => device.motion(0, x, y),
            Cmd::Leave => device.leave(),
            Cmd::Drop => device.drop(),
        }
    }
}

impl GlobalDispatch<WlSeat, ()> for Fake {
    fn bind(_: &mut Self, _: &DisplayHandle, _: &Client, r: New<WlSeat>, _: &(), init: &mut DataInit<'_, Self>) {
        init.init(r, ());
    }
}
impl Dispatch<WlSeat, ()> for Fake {
    fn request(_: &mut Self, _: &Client, _: &WlSeat, _: wl_seat::Request, _: &(), _: &DisplayHandle, _: &mut DataInit<'_, Self>) {}
}

impl GlobalDispatch<WlDataDeviceManager, ()> for Fake {
    fn bind(_: &mut Self, _: &DisplayHandle, _: &Client, r: New<WlDataDeviceManager>, _: &(), init: &mut DataInit<'_, Self>) {
        init.init(r, ());
    }
}
impl Dispatch<WlDataDeviceManager, ()> for Fake {
    fn request(state: &mut Self, _: &Client, _: &WlDataDeviceManager, req: wl_data_device_manager::Request, _: &(), _: &DisplayHandle, init: &mut DataInit<'_, Self>) {
        match req {
            wl_data_device_manager::Request::GetDataDevice { id, .. } => {
                let d = init.init(id, ());
                state.devices.push(d);
                state.note(Seen::Device);
            }
            wl_data_device_manager::Request::CreateDataSource { id } => {
                init.init(id, ());
            }
            _ => {}
        }
    }
}

impl Dispatch<WlDataDevice, ()> for Fake {
    fn request(_: &mut Self, _: &Client, _: &WlDataDevice, _: wl_data_device::Request, _: &(), _: &DisplayHandle, _: &mut DataInit<'_, Self>) {}
}
impl Dispatch<WlDataSource, ()> for Fake {
    fn request(_: &mut Self, _: &Client, _: &WlDataSource, _: wl_data_source::Request, _: &(), _: &DisplayHandle, _: &mut DataInit<'_, Self>) {}
}

impl Dispatch<WlDataOffer, ()> for Fake {
    fn request(state: &mut Self, _: &Client, _: &WlDataOffer, req: wl_data_offer::Request, _: &(), _: &DisplayHandle, _: &mut DataInit<'_, Self>) {
        use wl_data_offer::Request as R;
        match req {
            R::Accept { mime_type, .. } => state.note(Seen::Accept(mime_type)),
            R::SetActions { dnd_actions, preferred_action } => {
                state.note(Seen::SetActions { dnd: dnd_actions.into_result().unwrap(), preferred: preferred_action.into_result().unwrap() });
            }
            R::Receive { mime_type, fd } => {
                let answer = state.answers.lock().unwrap().get(&mime_type).cloned().unwrap_or(Answer::Silent);
                state.note(Seen::Receive(mime_type));
                match answer {
                    Answer::Data(bytes) => {
                        // the glue reads it all (small) and sees the pipe close
                        let mut f = std::fs::File::from(fd);
                        f.write_all(&bytes).unwrap();
                    }
                    Answer::Silent => state.held.push(fd),
                }
            }
            R::Finish => state.note(Seen::Finish),
            R::Destroy => state.note(Seen::Destroy),
            _ => {}
        }
    }
}

// The glue never binds wl_compositor: the test does, to have a surface for `enter`.
impl GlobalDispatch<wl_compositor::WlCompositor, ()> for Fake {
    fn bind(_: &mut Self, _: &DisplayHandle, _: &Client, r: New<wl_compositor::WlCompositor>, _: &(), init: &mut DataInit<'_, Self>) {
        init.init(r, ());
    }
}
impl Dispatch<wl_compositor::WlCompositor, ()> for Fake {
    fn request(state: &mut Self, _: &Client, _: &wl_compositor::WlCompositor, req: wl_compositor::Request, _: &(), _: &DisplayHandle, init: &mut DataInit<'_, Self>) {
        if let wl_compositor::Request::CreateSurface { id } = req {
            state.surface = Some(init.init(id, ()));
        }
    }
}
impl Dispatch<wl_surface::WlSurface, ()> for Fake {
    fn request(_: &mut Self, _: &Client, _: &wl_surface::WlSurface, _: wl_surface::Request, _: &(), _: &DisplayHandle, _: &mut DataInit<'_, Self>) {}
}

struct NoData;
impl wayland_server::backend::ClientData for NoData {}

/// The client side of the test: binds wl_compositor to make the surface a drag enters.
struct Probe;
impl ClientDispatch<c_registry::WlRegistry, GlobalListContents> for Probe {
    fn event(_: &mut Self, _: &c_registry::WlRegistry, _: c_registry::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {}
}
impl ClientDispatch<c_compositor::WlCompositor, ()> for Probe {
    fn event(_: &mut Self, _: &c_compositor::WlCompositor, _: c_compositor::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}
impl ClientDispatch<c_surface::WlSurface, ()> for Probe {
    fn event(_: &mut Self, _: &c_surface::WlSurface, _: c_surface::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

/// A running pretend compositor with the glue listening on its other end.
pub struct Rig {
    cmds: Sender<Cmd>,
    log: Log,
    answers: Arc<Mutex<HashMap<String, Answer>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    events: Receiver<DropEvent>,
    _surface: c_surface::WlSurface,
}

impl Rig {
    /// Start the compositor and the glue; returns when the glue has its data device.
    pub fn start(limits: Limits) -> Rig {
        let (server_end, client_end) = UnixStream::pair().unwrap();
        let log: Log = Arc::default();
        let answers: Arc<Mutex<HashMap<String, Answer>>> = Arc::default();
        let stop = Arc::new(AtomicBool::new(false));
        let (cmds, cmd_rx) = channel();
        let thread = {
            let (log, answers, stop) = (log.clone(), answers.clone(), stop.clone());
            std::thread::spawn(move || {
                let mut display: Display<Fake> = Display::new().unwrap();
                let dh = display.handle();
                dh.create_global::<Fake, WlSeat, _>(5, ());
                dh.create_global::<Fake, WlDataDeviceManager, _>(3, ());
                dh.create_global::<Fake, wl_compositor::WlCompositor, _>(4, ());
                display.handle().insert_client(server_end, Arc::new(NoData)).unwrap();
                let mut fake = Fake { log, answers, devices: vec![], surface: None, held: vec![], serial: 0 };
                while !stop.load(Ordering::Relaxed) {
                    while let Ok(cmd) = cmd_rx.try_recv() {
                        fake.apply(&dh, cmd);
                    }
                    if display.dispatch_clients(&mut fake).is_err() || display.flush_clients().is_err() {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(2));
                }
            })
        };
        let conn = Connection::from_socket(client_end).unwrap();
        let (tx, events) = channel();
        let (c2, sink) = (conn.clone(), Sink::new(tx));
        std::thread::spawn(move || listen(c2, sink, limits));
        // a surface for the drags to enter
        let (globals, mut queue) = registry_queue_init::<Probe>(&conn).unwrap();
        let qh = queue.handle();
        let compositor: c_compositor::WlCompositor = globals.bind(&qh, 1..=4, ()).unwrap();
        let surface = compositor.create_surface(&qh, ());
        queue.roundtrip(&mut Probe).unwrap();
        let rig = Rig { cmds, log, answers, stop, thread: Some(thread), events, _surface: surface };
        rig.wait_for("the data device", |l| l.contains(&Seen::Device));
        rig
    }

    /// What the compositor writes when the glue asks for `mime`.
    pub fn answer(&self, mime: &str, a: Answer) {
        self.answers.lock().unwrap().insert(mime.to_string(), a);
    }

    /// A drag with these kinds enters the window.
    pub fn enter(&self, mimes: &[&str], x: f64, y: f64) {
        self.cmds.send(Cmd::Enter { mimes: mimes.iter().map(|m| m.to_string()).collect(), x, y }).unwrap();
    }

    pub fn motion(&self, x: f64, y: f64) {
        self.cmds.send(Cmd::Motion { x, y }).unwrap();
    }

    pub fn leave(&self) {
        self.cmds.send(Cmd::Leave).unwrap();
    }

    pub fn drop_it(&self) {
        self.cmds.send(Cmd::Drop).unwrap();
    }

    /// Everything the glue did so far.
    pub fn seen(&self) -> Vec<Seen> {
        self.log.lock().unwrap().clone()
    }

    /// Wait until `cond` holds for what the glue did so far; fails after a few seconds.
    pub fn wait_for(&self, what: &str, cond: impl Fn(&[Seen]) -> bool) {
        let t = Instant::now();
        while !cond(&self.seen()) {
            assert!(t.elapsed() < WAIT, "timed out waiting for {what}; saw {:?}", self.seen());
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// The next event the glue sends; fails after a few seconds.
    pub fn event(&self) -> DropEvent {
        self.events.recv_timeout(WAIT).expect("a drop event")
    }

    /// True when no event comes within a short time.
    pub fn quiet(&self) -> bool {
        self.events.recv_timeout(Duration::from_millis(300)).is_err()
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
