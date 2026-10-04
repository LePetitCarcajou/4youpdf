//! Page images: the thumbnails of the grid and the page view, sized to the
//! window. The interface asks for "page N of the open document, W pixels
//! wide" and gets a PNG; nothing else about the renderer reaches it. Today
//! the renderer is PDFium through `pdfium-render` (ADR 0005), loaded at run
//! time when its shared library is found; without it the application still
//! opens, reorders and saves, with blank placeholders.
//!
//! PDFium does not run in the process of the window (ADR 0008). It runs in
//! a worker, this same executable started in another mode ([`worker`]),
//! which this module starts, talks to over its standard input and output
//! ([`protocol`]), watches and starts again:
//!
//! - the worker draws, and answers with raw pixels; the window checks
//!   their dimensions and encodes the PNG itself;
//! - a worker that dies, stays silent longer than a request may take or
//!   answers something else than what was asked is stopped; the request
//!   fails, and the next one starts another worker, to which the document
//!   is sent again;
//! - a page that brought the worker down twice is refused from then on,
//!   until another document comes; too many restarts in a short time, and
//!   rendering is off until the application is started again.
//!
//! One request at a time reaches the worker: PDFium is not thread-safe,
//! and a page as wide as the window takes a while. So the page view sends
//! one request at a time, its current page first, and thumbnails wait
//! until it has nothing left to draw, then pass one by one while the panel
//! is shown: a page asked for while browsing waits behind one request at
//! most, a neighbour or a thumbnail.

mod pdfium;
mod png;
pub mod protocol;
pub mod worker;

use std::collections::{HashMap, VecDeque};
use std::hash::{BuildHasher, Hasher, RandomState};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, TryLockError};
use std::thread;
use std::time::{Duration, Instant};

use image::{DynamicImage, RgbaImage};
use serde::Serialize;

use protocol::{ProtocolError, Reply, MAX_WIDTH, MIN_WIDTH, VERSION};

/// Whether thumbnails can be drawn, and why not when they cannot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Status {
    /// Whether pages are drawn.
    pub available: bool,
    /// Where PDFium was loaded from, or why pages are not drawn.
    pub detail: String,
}

/// How long the window waits for the worker and how often it starts it
/// again (ADR 0008, « Plafonds »).
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// How long the worker may take to answer the handshake.
    pub handshake: Duration,
    /// How long the worker may take to load a document or to draw a page.
    pub request: Duration,
    /// How many times the worker may be started again within
    /// `restart_window` before rendering is off for the session.
    pub restarts: usize,
    /// The time over which restarts are counted.
    pub restart_window: Duration,
    /// How long the worker has to exit once its input is closed, before it
    /// is killed.
    pub farewell: Duration,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            handshake: Duration::from_secs(10),
            request: Duration::from_secs(30),
            restarts: 8,
            restart_window: Duration::from_secs(60),
            farewell: Duration::from_secs(2),
        }
    }
}

/// How many times a page, or the opening of a document, may bring the
/// worker down before it is refused.
const FALLS: u8 = 2;

/// What starts a worker.
pub trait Launch: Send + Sync {
    /// Start one, and give the ends of its channel.
    fn launch(&self) -> io::Result<Link>;
}

/// A worker just started: where to write to it, where to read from it, and
/// its process.
pub struct Link {
    /// Its standard input.
    pub input: Box<dyn Write + Send>,
    /// Its standard output.
    pub output: Box<dyn Read + Send>,
    /// The process, to watch and to stop.
    pub process: Box<dyn Process>,
}

/// The process of a worker.
pub trait Process: Send {
    /// Its identifier for the system.
    fn id(&self) -> u32;
    /// Whether it has ended; never blocks.
    fn exited(&mut self) -> bool;
    /// Stop it now and wait until it is gone.
    fn kill(&mut self);
}

impl Process for Child {
    fn id(&self) -> u32 {
        Child::id(self)
    }

    fn exited(&mut self) -> bool {
        // An error leaves nothing to wait for either.
        !matches!(self.try_wait(), Ok(None))
    }

    fn kill(&mut self) {
        let _ = Child::kill(self);
        let _ = self.wait();
    }
}

/// The worker as the application runs it: an executable started with
/// [`worker::ARGUMENT`] alone, its standard input and output piped, its
/// standard error dropped, and no console window on Windows. Nothing about
/// a document goes on its command line or in its environment.
pub struct Executable(PathBuf);

impl Executable {
    /// The worker `program` becomes with [`worker::ARGUMENT`]: `fyp-app`.
    pub fn new(program: impl Into<PathBuf>) -> Executable {
        Executable(program.into())
    }

    /// The executable that runs: the application starts itself again.
    pub fn this() -> io::Result<Executable> {
        std::env::current_exe().map(Executable)
    }
}

impl Launch for Executable {
    fn launch(&self) -> io::Result<Link> {
        let mut command = Command::new(&self.0);
        command
            .arg(worker::ARGUMENT)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // CREATE_NO_WINDOW: a development build is a console program,
            // and its worker must not open a console of its own.
            command.creation_flags(0x0800_0000);
        }
        let mut child = command.spawn()?;
        let (Some(input), Some(output)) = (child.stdin.take(), child.stdout.take()) else {
            Process::kill(&mut child);
            return Err(io::Error::other("canal du moteur de rendu absent"));
        };
        Ok(Link {
            input: Box::new(input),
            output: Box::new(output),
            process: Box::new(child),
        })
    }
}

/// The launcher of a service that never had a worker.
struct NoWorker;

impl Launch for NoWorker {
    fn launch(&self) -> io::Result<Link> {
        Err(io::Error::other("aucun moteur de rendu"))
    }
}

/// What goes to the worker: a frame, or the bytes of a document, shared
/// with the session rather than copied.
enum Outgoing {
    Owned(Vec<u8>),
    Shared(Arc<Vec<u8>>),
}

/// A worker that answered the handshake, as the window sees it. Two
/// threads own the ends of its channel, so that neither a worker that
/// stops reading nor one that stops answering holds the window: what is
/// waited for is `inbox`, never a pipe.
struct Worker {
    /// To the thread that writes to the worker; dropped, the thread closes
    /// the input of the worker, which then exits.
    outbox: Sender<Outgoing>,
    /// From the thread that reads the worker: its replies, and last the
    /// reason the reading stopped.
    inbox: Receiver<Result<Reply, ProtocolError>>,
    /// The document this worker was sent last, and whether it loaded it.
    loaded: Option<(u64, Result<(), String>)>,
}

/// Why an exchange with the worker ended without a reply to the request.
/// In each case the worker is stopped.
#[derive(Debug)]
enum Failure {
    /// It is gone: crashed, killed, or its channel closed.
    Stopped,
    /// It did not answer in time.
    Silent(Duration),
    /// It answered something that is not a reply to the request.
    Lied(String),
}

impl Failure {
    fn message(&self) -> String {
        match self {
            Failure::Stopped => "le moteur de rendu s'est arrêté ; il sera relancé".into(),
            Failure::Silent(patience) => format!(
                "le moteur de rendu n'a pas répondu en {} s ; il a été arrêté et sera relancé",
                patience.as_secs().max(1)
            ),
            Failure::Lied(what) => format!(
                "le moteur de rendu a donné une réponse inattendue ({what}) ; il a été arrêté et sera relancé"
            ),
        }
    }
}

impl Worker {
    fn send(&self, chunk: Outgoing) -> Result<(), Failure> {
        self.outbox.send(chunk).map_err(|_| Failure::Stopped)
    }

    fn receive(&self, patience: Duration) -> Result<Reply, Failure> {
        match self.inbox.recv_timeout(patience) {
            Ok(Ok(reply)) => Ok(reply),
            Ok(Err(ProtocolError::Closed | ProtocolError::Truncated | ProtocolError::Io(_)))
            | Err(RecvTimeoutError::Disconnected) => Err(Failure::Stopped),
            Ok(Err(lie)) => Err(Failure::Lied(lie.to_string())),
            Err(RecvTimeoutError::Timeout) => Err(Failure::Silent(patience)),
        }
    }
}

/// Write to the worker what `frames` brings, until it ends or the worker
/// stops reading.
fn pump_out(mut input: Box<dyn Write + Send>, frames: &Receiver<Outgoing>) {
    for chunk in frames {
        let bytes: &[u8] = match &chunk {
            Outgoing::Owned(bytes) => bytes,
            Outgoing::Shared(bytes) => bytes,
        };
        if input.write_all(bytes).and_then(|()| input.flush()).is_err() {
            break;
        }
    }
}

/// Read the replies of the worker into `replies`, until one cannot be
/// read. `replies` holds one at most: a worker that answers what it was
/// not asked fills its pipe, not the memory of the window.
fn pump_in(mut output: Box<dyn Read + Send>, replies: &SyncSender<Result<Reply, ProtocolError>>) {
    loop {
        let reply = protocol::read_reply(&mut output);
        let last = reply.is_err();
        if replies.send(reply).is_err() || last {
            break;
        }
    }
}

/// What a request was doing when the worker fell.
#[derive(Debug, Clone, Copy)]
enum Phase {
    /// Loading the document.
    Opening,
    /// Drawing this page (0-based).
    Page(u32),
}

/// How many times the document being shown brought the worker down.
#[derive(Default)]
struct Strikes {
    document: Option<u64>,
    opening: u8,
    pages: HashMap<u32, u8>,
}

impl Strikes {
    /// The request is for `document`: another one than before starts
    /// from nothing.
    fn concern(&mut self, document: u64) {
        if self.document != Some(document) {
            *self = Strikes {
                document: Some(document),
                ..Strikes::default()
            };
        }
    }

    fn fall(&mut self, phase: Phase) {
        let count = match phase {
            Phase::Opening => &mut self.opening,
            Phase::Page(page) => self.pages.entry(page).or_default(),
        };
        *count = count.saturating_add(1);
    }

    /// Why `page` (0-based) is not drawn any more, if it is not.
    fn refusal(&self, page: u32) -> Option<String> {
        if self.opening >= FALLS {
            return Some(
                "ce document a arrêté le moteur de rendu deux fois à l'ouverture ; ses pages ne sont plus dessinées"
                    .into(),
            );
        }
        (self.pages.get(&page).copied().unwrap_or(0) >= FALLS).then(|| {
            format!(
                "la page {} a arrêté le moteur de rendu deux fois ; elle n'est plus dessinée",
                u64::from(page) + 1
            )
        })
    }
}

/// What the worker answers to a request for a page: the height and the
/// RGBA pixels of its image, or why the page cannot be drawn.
type Drawn = Result<(u32, Vec<u8>), String>;

/// What requests share, one at a time.
#[derive(Default)]
struct State {
    worker: Option<Worker>,
    /// When the worker was started again, over the last
    /// [`Limits::restart_window`].
    restarts: VecDeque<Instant>,
    strikes: Strikes,
}

/// The number a request carries and its reply echoes. Drawn at random, so
/// that a reply written ahead of a request cannot carry its number: the
/// keys of [`RandomState`] are the randomness the standard library has.
fn sequence() -> u64 {
    RandomState::new().build_hasher().finish()
}

/// Why a worker did not start.
enum Start {
    /// Starting another would give the same: no library, another protocol.
    Never(String),
    /// This one failed; another may be tried.
    Failed(String),
}

/// The page service of the window: it owns the worker.
pub struct RenderService {
    launcher: Box<dyn Launch>,
    candidates: Vec<PathBuf>,
    limits: Limits,
    /// Apart from `state`, so that the status is read while a page is
    /// being drawn.
    status: Mutex<Status>,
    state: Mutex<State>,
    /// The process of the worker, apart from `state` so that closing the
    /// application can stop it under a request in flight.
    process: Mutex<Option<Box<dyn Process>>>,
    closing: AtomicBool,
    drawing: AtomicBool,
}

impl RenderService {
    /// Start the worker: this executable, started again as one
    /// ([`Executable::this`]). It looks for the PDFium library in
    /// `candidates` (directories), then, except on Windows, in the system
    /// library path.
    pub fn start(candidates: &[PathBuf]) -> RenderService {
        match Executable::this() {
            Ok(executable) => {
                RenderService::start_with(Box::new(executable), candidates, Limits::default())
            }
            Err(e) => {
                RenderService::unavailable(&format!("le moteur de rendu n'a pas démarré : {e}"))
            }
        }
    }

    /// Start the worker `launcher` gives, within `limits`. A worker that
    /// finds no library, or speaks another protocol, leaves a service that
    /// says why and draws nothing. One that does not start, or dies or stays
    /// silent before it shook hands, is started again when the first page
    /// is asked for, like a worker that fell.
    pub fn start_with(
        launcher: Box<dyn Launch>,
        candidates: &[PathBuf],
        limits: Limits,
    ) -> RenderService {
        let service = RenderService {
            launcher,
            candidates: candidates.to_vec(),
            limits,
            status: Mutex::new(Status {
                available: false,
                detail: String::new(),
            }),
            state: Mutex::new(State::default()),
            process: Mutex::new(None),
            closing: AtomicBool::new(false),
            drawing: AtomicBool::new(false),
        };
        let started = service.launch(&mut service.state());
        match started {
            Ok(detail) => service.set_status(true, detail),
            Err(Start::Never(detail)) => service.set_status(false, detail),
            // This worker failed; the first page asked for starts another,
            // as after any fall, within the same count of restarts.
            Err(Start::Failed(detail)) => service.set_status(true, detail),
        }
        service
    }

    /// A service without a worker, which draws nothing and says `detail`.
    pub fn unavailable(detail: &str) -> RenderService {
        RenderService {
            launcher: Box::new(NoWorker),
            candidates: Vec::new(),
            limits: Limits::default(),
            status: Mutex::new(Status {
                available: false,
                detail: detail.to_string(),
            }),
            state: Mutex::new(State::default()),
            process: Mutex::new(None),
            closing: AtomicBool::new(false),
            drawing: AtomicBool::new(false),
        }
    }

    /// Whether pages are drawn, as of now: rendering that was available
    /// may stop being so (too many restarts).
    pub fn status(&self) -> Status {
        self.status
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The process identifier of the worker, when one runs.
    pub fn worker_pid(&self) -> Option<u32> {
        self.process().as_ref().map(|process| process.id())
    }

    /// Whether the worker is drawing a page at this instant.
    pub fn is_drawing(&self) -> bool {
        self.drawing.load(Ordering::SeqCst)
    }

    /// Render one page (0-based) as PNG. Blocks until the worker answers,
    /// or for as long as a request may take: call it off the interface
    /// thread. The PNG is encoded here, from the pixels of the worker.
    pub fn render(
        &self,
        document_id: u64,
        bytes: Arc<Vec<u8>>,
        password: &str,
        page: usize,
        width: u32,
    ) -> Result<Vec<u8>, String> {
        let width = width.clamp(MIN_WIDTH, MAX_WIDTH);
        let (height, pixels) = self.draw(document_id, &bytes, password, page, width)?;
        // Outside the turn of this request: the worker may already draw
        // the next page.
        let image = RgbaImage::from_raw(width, height, pixels)
            .ok_or_else(|| "image incomplète".to_string())?;
        png::encode_png(&DynamicImage::ImageRgba8(image))
    }

    /// Stop the worker and wait until it is gone: its input is closed, and
    /// it is killed if it has not exited after [`Limits::farewell`]. A
    /// request in flight fails at once. Nothing is drawn afterwards.
    pub fn shutdown(&self) {
        self.closing.store(true, Ordering::SeqCst);
        let mut state = match self.state.try_lock() {
            Ok(state) => state,
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(TryLockError::WouldBlock) => {
                // A request is in flight: it ends when its worker does.
                if let Some(process) = self.process().as_mut() {
                    process.kill();
                }
                self.state()
            }
        };
        if let Some(worker) = state.worker.take() {
            self.retire(worker, self.limits.farewell);
        }
        if self.status().available {
            self.set_status(false, CLOSING.into());
        }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn process(&self) -> MutexGuard<'_, Option<Box<dyn Process>>> {
        self.process.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn set_status(&self, available: bool, detail: String) {
        *self.status.lock().unwrap_or_else(PoisonError::into_inner) = Status { available, detail };
    }

    /// The height and the RGBA pixels of page `page` (0-based), `width`
    /// pixels wide: the turn of one request with the worker.
    fn draw(
        &self,
        document: u64,
        bytes: &Arc<Vec<u8>>,
        password: &str,
        page: usize,
        width: u32,
    ) -> Drawn {
        let mut state = self.state();
        let status = self.status();
        if !status.available {
            return Err(status.detail);
        }
        if self.closing.load(Ordering::SeqCst) {
            return Err(CLOSING.into());
        }
        let page = u32::try_from(page)
            .map_err(|_| format!("page {} hors de portée", page.saturating_add(1)))?;
        state.strikes.concern(document);
        if let Some(refusal) = state.strikes.refusal(page) {
            return Err(refusal);
        }
        self.ensure_worker(&mut state)?;
        let Some(mut worker) = state.worker.take() else {
            return Err(Failure::Stopped.message());
        };
        match self.ask(&mut worker, document, bytes, password, page, width) {
            Ok(answer) => {
                state.worker = Some(worker);
                answer
            }
            Err((phase, failure)) => {
                state.strikes.fall(phase);
                self.retire(worker, Duration::ZERO);
                Err(failure.message())
            }
        }
    }

    /// Send `worker` the document unless it holds it, then ask it for the
    /// page. The inner error is the worker's answer that the document or
    /// the page cannot be drawn; the outer one, that the worker fell.
    fn ask(
        &self,
        worker: &mut Worker,
        document: u64,
        bytes: &Arc<Vec<u8>>,
        password: &str,
        page: u32,
        width: u32,
    ) -> Result<Drawn, (Phase, Failure)> {
        let held = match &worker.loaded {
            Some((id, outcome)) if *id == document => Some(outcome.clone()),
            _ => None,
        };
        let outcome = if let Some(outcome) = held {
            outcome
        } else {
            let opening = |failure| (Phase::Opening, failure);
            let sequence = sequence();
            let Ok(head) = protocol::document_head(sequence, document, password, bytes.len())
            else {
                return Ok(Err("document trop grand pour le moteur de rendu".into()));
            };
            worker.loaded = None;
            worker.send(Outgoing::Owned(head)).map_err(opening)?;
            worker
                .send(Outgoing::Shared(Arc::clone(bytes)))
                .map_err(opening)?;
            let outcome = match worker.receive(self.limits.request).map_err(opening)? {
                Reply::Opened { sequence: echoed } if echoed == sequence => Ok(()),
                Reply::Failed {
                    sequence: echoed,
                    message,
                } if echoed == sequence => Err(message),
                _ => return Err(opening(Failure::Lied(OTHER_REQUEST.into()))),
            };
            // A document PDFium refuses is refused once, not once per page.
            worker.loaded = Some((document, outcome.clone()));
            outcome
        };
        if let Err(message) = outcome {
            return Ok(Err(message));
        }
        let drawing = |failure| (Phase::Page(page), failure);
        let sequence = sequence();
        worker
            .send(Outgoing::Owned(protocol::draw_frame(
                sequence, document, page, width,
            )))
            .map_err(drawing)?;
        self.drawing.store(true, Ordering::SeqCst);
        let reply = worker.receive(self.limits.request);
        self.drawing.store(false, Ordering::SeqCst);
        match reply.map_err(drawing)? {
            Reply::Bitmap {
                sequence: echoed,
                width: drawn,
                height,
                pixels,
            } if echoed == sequence => {
                if drawn == width {
                    Ok(Ok((height, pixels)))
                } else {
                    Err(drawing(Failure::Lied(format!(
                        "image de {drawn} pixels de large pour {width} demandés"
                    ))))
                }
            }
            Reply::Failed {
                sequence: echoed,
                message,
            } if echoed == sequence => Ok(Err(message)),
            _ => Err(drawing(Failure::Lied(OTHER_REQUEST.into()))),
        }
    }

    /// Leave `state` with a worker that runs, starting one if the last is
    /// gone, unless it was started too often already.
    fn ensure_worker(&self, state: &mut State) -> Result<(), String> {
        if let Some(worker) = state.worker.take() {
            let exited = self
                .process()
                .as_mut()
                .is_none_or(|process| process.exited());
            if !exited {
                state.worker = Some(worker);
                return Ok(());
            }
            // It died between two requests: none of them is to blame.
            self.retire(worker, Duration::ZERO);
        }
        let now = Instant::now();
        while state
            .restarts
            .front()
            .is_some_and(|then| now.duration_since(*then) > self.limits.restart_window)
        {
            state.restarts.pop_front();
        }
        if state.restarts.len() >= self.limits.restarts {
            let detail = format!(
                "le moteur de rendu s'est arrêté trop souvent ({} relances en moins de {} s) ; les aperçus sont indisponibles jusqu'au redémarrage de 4YouPDF",
                state.restarts.len(),
                self.limits.restart_window.as_secs().max(1)
            );
            self.set_status(false, detail.clone());
            return Err(detail);
        }
        // Closing may have begun while this request waited for its turn,
        // or stopped the worker just found dead: nothing is started then.
        if self.closing.load(Ordering::SeqCst) {
            return Err(CLOSING.into());
        }
        state.restarts.push_back(now);
        match self.launch(state) {
            Ok(_) if self.closing.load(Ordering::SeqCst) => {
                // Closing began during the handshake, before it could see
                // this worker: it is let go of here, and asked nothing.
                if let Some(worker) = state.worker.take() {
                    self.retire(worker, Duration::ZERO);
                }
                Err(CLOSING.into())
            }
            Ok(detail) => {
                self.set_status(true, detail);
                Ok(())
            }
            Err(Start::Never(detail)) => {
                self.set_status(false, detail.clone());
                Err(detail)
            }
            Err(Start::Failed(detail)) => Err(detail),
        }
    }

    /// Start a worker and shake hands with it; the text says where it
    /// found PDFium.
    fn launch(&self, state: &mut State) -> Result<String, Start> {
        let failed = |why: &dyn std::fmt::Display| {
            Start::Failed(format!("le moteur de rendu n'a pas démarré : {why}"))
        };
        let hello = protocol::hello_frame(VERSION, &self.candidates).map_err(|e| failed(&e))?;
        let Link {
            input,
            output,
            process,
        } = self.launcher.launch().map_err(|e| failed(&e))?;
        if let Some(mut stray) = self.process().replace(process) {
            // Never two workers: one that was not let go of ends here.
            stray.kill();
        }
        let (outbox, frames) = mpsc::channel();
        let (replies, inbox) = mpsc::sync_channel(1);
        let worker = Worker {
            outbox,
            inbox,
            loaded: None,
        };
        let threads = thread::Builder::new()
            .name("render-out".into())
            .spawn(move || pump_out(input, &frames))
            .and_then(|_| {
                thread::Builder::new()
                    .name("render-in".into())
                    .spawn(move || pump_in(output, &replies))
            });
        if let Err(e) = threads {
            self.retire(worker, Duration::ZERO);
            return Err(failed(&e));
        }
        let reply = worker
            .send(Outgoing::Owned(hello))
            .and_then(|()| worker.receive(self.limits.handshake));
        match reply {
            Ok(Reply::Ready {
                version: VERSION,
                available: true,
                detail,
            }) => {
                state.worker = Some(worker);
                Ok(detail)
            }
            Ok(Reply::Ready {
                version: VERSION,
                available: false,
                detail,
            }) => {
                // It has nothing to draw with, and exits by itself.
                self.retire(worker, self.limits.farewell);
                Err(Start::Never(detail))
            }
            Ok(Reply::Ready { version, .. } | Reply::Refused { version }) => {
                self.retire(worker, Duration::ZERO);
                Err(Start::Never(format!(
                    "le moteur de rendu parle le protocole {version}, la fenêtre le protocole {VERSION}"
                )))
            }
            Ok(_) => {
                self.retire(worker, Duration::ZERO);
                Err(failed(&"poignée de main sans réponse"))
            }
            Err(failure) => {
                self.retire(worker, Duration::ZERO);
                Err(failed(&match failure {
                    Failure::Stopped => "il s'est arrêté pendant la poignée de main".to_string(),
                    Failure::Silent(patience) => format!(
                        "pas de réponse à la poignée de main en {} s",
                        patience.as_secs().max(1)
                    ),
                    Failure::Lied(what) => format!("poignée de main inattendue ({what})"),
                }))
            }
        }
    }

    /// Let go of `worker`: close its input, give its process `patience` to
    /// exit, then kill it. The process is waited for in every case.
    fn retire(&self, worker: Worker, patience: Duration) {
        drop(worker);
        let Some(mut process) = self.process().take() else {
            return;
        };
        let deadline = Instant::now() + patience;
        while !process.exited() {
            if Instant::now() >= deadline {
                process.kill();
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
    }
}

impl Drop for RenderService {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// What a request is told once the application closes.
const CLOSING: &str = "le moteur de rendu est arrêté : l'application se ferme";

/// What the worker is told to have done when its reply is not to the
/// request.
const OTHER_REQUEST: &str = "réponse à une autre demande";

/// Where the PDFium library may be, in this order: the directory named by
/// `FYP_PDFIUM_DIR`; the directory of the executable, where the installer
/// and the portable archive put it; and `development`, the `app/pdfium/`
/// of the checkout a development build was compiled from
/// (`tools/fetch_pdfium.py`), which a packaged build does not give.
pub fn library_candidates(development: Option<PathBuf>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(dir) = std::env::var_os("FYP_PDFIUM_DIR") {
        dirs.push(PathBuf::from(dir));
    }
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
    {
        dirs.push(dir);
    }
    dirs.extend(development);
    dirs
}

#[cfg(test)]
#[allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]
mod tests;
