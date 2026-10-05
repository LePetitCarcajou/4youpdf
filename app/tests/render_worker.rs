//! The rendering worker for real (ADR 0008): the `fyp-app` executable
//! started in its worker mode, by the service of the window or by hand.
//! What needs PDFium is skipped, and says so, when `tools/fetch_pdfium.py`
//! has not run; the refusals and the end of the worker are checked in every
//! case.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use fyp_app::render::protocol::{self, Reply, Request, VERSION};
use fyp_app::render::{worker, Executable, Launch, Limits, RenderService};
use fyp_core::document::Document;
use fyp_core::ops;

const FYP_APP: &str = env!("CARGO_BIN_EXE_fyp-app");

fn pdfium_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("pdfium")
}

fn fixture(name: &str) -> Arc<Vec<u8>> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../tests/fixtures")
        .join(name);
    Arc::new(std::fs::read(path).unwrap())
}

/// The service of the window over the real worker, or `None`, said on the
/// standard error, when PDFium is not there to draw with.
fn service() -> Option<RenderService> {
    let service = RenderService::start_with(
        Box::new(Executable::new(FYP_APP)),
        &[pdfium_dir()],
        Limits::default(),
    );
    let status = service.status();
    if status.available {
        Some(service)
    } else {
        eprintln!("PDFium not fetched: skipped ({})", status.detail);
        None
    }
}

/// A one-page PDF with the media box and the content given.
fn pdf(media_box: &str, content: &str) -> Arc<Vec<u8>> {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [{media_box}] /Contents 4 0 R >>"),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ),
    ];
    let mut file = String::from("%PDF-1.7\n");
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(file.len());
        write!(file, "{} 0 obj\n{object}\nendobj\n", index + 1).unwrap();
    }
    let xref = file.len();
    write!(file, "xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).unwrap();
    for offset in offsets {
        writeln!(file, "{offset:010} 00000 n ").unwrap();
    }
    write!(
        file,
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        objects.len() + 1
    )
    .unwrap();
    Arc::new(file.into_bytes())
}

/// A page that takes PDFium a while: thousands of filled curves.
fn heavy_page() -> Arc<Vec<u8>> {
    let mut content = String::new();
    for i in 0..40_000_u32 {
        let (x, y) = (i * 7 % 590, i * 13 % 840);
        writeln!(
            content,
            "{} {} {} rg {x} {y} m {} {} {} {} {} {} c f",
            f64::from(i % 10) / 10.0,
            f64::from(i % 7) / 7.0,
            f64::from(i % 3) / 3.0,
            x + 200,
            y + 50,
            x + 50,
            y + 300,
            x + 5,
            y + 5
        )
        .unwrap();
    }
    pdf("0 0 595 842", &content)
}

fn kill(pid: u32) {
    let pid = pid.to_string();
    let status = if cfg!(windows) {
        Command::new("taskkill").args(["/F", "/PID", &pid]).output()
    } else {
        Command::new("kill").args(["-9", &pid]).output()
    };
    status.expect("kill");
}

/// Whether a process `pid` exists.
fn alive(pid: u32) -> bool {
    if cfg!(windows) {
        let output = Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
            .output()
            .expect("tasklist");
        String::from_utf8_lossy(&output.stdout).contains(&format!("\"{pid}\""))
    } else {
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .output()
            .expect("kill -0")
            .status
            .success()
    }
}

fn gone_within(pid: u32, patience: Duration) -> bool {
    let started = Instant::now();
    while alive(pid) {
        if started.elapsed() > patience {
            return false;
        }
        thread::sleep(Duration::from_millis(20));
    }
    true
}

/// A worker started by hand, to be told anything.
struct ByHand {
    child: Child,
    input: Option<ChildStdin>,
    output: ChildStdout,
}

impl ByHand {
    fn start() -> ByHand {
        let mut child = Command::new(FYP_APP)
            .arg(worker::ARGUMENT)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("start the worker");
        ByHand {
            input: child.stdin.take(),
            output: child.stdout.take().unwrap(),
            child,
        }
    }

    fn tell(&mut self, request: &Request) {
        request.write_to(self.input.as_mut().unwrap()).unwrap();
    }

    fn hear(&mut self) -> Result<Reply, protocol::ProtocolError> {
        protocol::read_reply(&mut self.output)
    }

    fn close_input(&mut self) {
        self.input = None;
    }

    /// The exit status, if the worker ends within `patience`; it is killed
    /// otherwise.
    fn exit_within(&mut self, patience: Duration) -> Option<ExitStatus> {
        let started = Instant::now();
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return Some(status);
            }
            if started.elapsed() > patience {
                let _ = self.child.kill();
                let _ = self.child.wait();
                return None;
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for ByHand {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

const PATIENCE: Duration = Duration::from_secs(20);

/// Without the library where it is told to look, the worker says so and
/// the service draws nothing, in words (unless the system has a PDFium of
/// its own, which Windows never does).
#[test]
fn a_missing_library_is_reported_not_fatal() {
    let service = RenderService::start_with(
        Box::new(Executable::new(FYP_APP)),
        &[PathBuf::from("Z:/nowhere")],
        Limits::default(),
    );
    let status = service.status();
    if !status.available {
        assert!(status.detail.contains("introuvable"), "{}", status.detail);
        assert!(status.detail.contains("nowhere"), "{}", status.detail);
        let error = service
            .render(1, Arc::new(Vec::new()), "", 0, 100)
            .unwrap_err();
        assert_eq!(error, status.detail);
        // A worker with nothing to draw with does not stay.
        assert_eq!(service.worker_pid(), None);
    }
}

/// An executable that is not there: each page asked for says so, and the
/// service gives up once it has tried too often.
#[test]
fn a_worker_that_cannot_be_started_is_reported() {
    let service = RenderService::start_with(
        Box::new(Executable::new(pdfium_dir().join("no-such-program"))),
        &[],
        Limits::default(),
    );
    let status = service.status();
    assert!(
        status.detail.contains("n'a pas démarré"),
        "{}",
        status.detail
    );
    for _ in 0..Limits::default().restarts {
        let error = service
            .render(1, Arc::new(Vec::new()), "", 0, 100)
            .unwrap_err();
        assert!(error.contains("n'a pas démarré"), "{error}");
    }
    let error = service
        .render(1, Arc::new(Vec::new()), "", 0, 100)
        .unwrap_err();
    assert!(error.contains("trop souvent"), "{error}");
    assert!(!service.status().available);
    assert_eq!(service.worker_pid(), None);
}

/// The existing rendering checks, through the worker: the first page of a
/// fixture renders to a PNG of the requested width.
#[test]
fn renders_a_fixture_through_the_worker() {
    let Some(service) = service() else { return };
    assert!(service.status().detail.contains("PDFium chargé depuis"));
    let pid = service.worker_pid().expect("a worker runs");
    let bytes = fixture("encrypted-rc4.pdf");
    let png = service
        .render(7, Arc::clone(&bytes), "", 0, 120)
        .expect("render");
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    let image = image::load_from_memory(&png).expect("decode");
    assert_eq!(image.width(), 120);
    assert!(image.height() > 120, "A4 is taller than wide");
    // Second request on the same document: served from the cache.
    assert!(service.render(7, Arc::clone(&bytes), "", 0, 60).is_ok());
    // The page view asks for the width of the window: the image has
    // that width and the proportions of the page.
    let large = service
        .render(7, Arc::clone(&bytes), "", 0, 1400)
        .expect("render");
    let image = image::load_from_memory(&large).expect("decode");
    assert_eq!(image.width(), 1400);
    let ratio = f64::from(image.height()) / f64::from(image.width());
    assert!((ratio - 842.0 / 595.0).abs() < 0.01, "A4, got {ratio}");
    // The widest the interface may ask for: an A4 page fits the ceilings
    // of the protocol.
    let widest = service
        .render(7, Arc::clone(&bytes), "", 0, 9999)
        .expect("render at the widest");
    let image = image::load_from_memory(&widest).expect("decode");
    assert_eq!((image.width(), image.height()), (4096, 5796));
    // Once turned, as `session::rotate` turns it, the page is drawn on
    // its side: the renderer follows the `/Rotate` of the rewrite.
    let turned = {
        let document = Document::open_with_password(&bytes, b"").expect("open");
        ops::rotate(&document, &[0], 90).expect("rotate")
    };
    let png = service
        .render(8, Arc::new(turned), "", 0, 120)
        .expect("render");
    let image = image::load_from_memory(&png).expect("decode");
    assert_eq!(image.width(), 120);
    let ratio = f64::from(image.height()) / f64::from(image.width());
    assert!(
        (ratio - 595.0 / 842.0).abs() < 0.01,
        "A4 on its side, got {ratio}"
    );
    // Out-of-range page: an error that counts pages from 1, not a panic.
    let error = service.render(7, bytes, "", 9, 60).unwrap_err();
    assert!(error.contains("page 10"), "{error}");
    // Bytes that are no PDF: an error, once.
    let error = service
        .render(9, Arc::new(Vec::new()), "", 0, 60)
        .unwrap_err();
    assert!(error.contains("ne peut pas ouvrir"), "{error}");
    // None of this cost the worker its life.
    assert_eq!(service.worker_pid(), Some(pid));
}

/// The worker is killed while it draws: the request fails in words, and
/// the next one succeeds, served by another worker.
#[test]
fn a_worker_killed_while_it_draws_fails_the_request_and_the_next_one_succeeds() {
    let Some(service) = service() else { return };
    let service = Arc::new(service);
    let bytes = heavy_page();
    service
        .render(1, Arc::clone(&bytes), "", 0, 64)
        .expect("the page renders");
    // The kill has to land while the page is being drawn; a page drawn
    // before it lands is asked again.
    let mut failed = None;
    for _ in 0..10 {
        let pid = service.worker_pid().expect("a worker runs");
        let request = {
            let (service, bytes) = (Arc::clone(&service), Arc::clone(&bytes));
            thread::spawn(move || service.render(1, bytes, "", 0, 4096))
        };
        let started = Instant::now();
        while !service.is_drawing() && started.elapsed() < PATIENCE {
            thread::yield_now();
        }
        kill(pid);
        let outcome = request.join().expect("no panic");
        assert!(gone_within(pid, PATIENCE));
        if let Err(error) = outcome {
            failed = Some((pid, error));
            break;
        }
    }
    let (killed, error) = failed.expect("a kill lands while the page is drawn");
    assert_eq!(error, "le moteur de rendu s'est arrêté ; il sera relancé");
    assert_eq!(service.worker_pid(), None, "started again on demand only");
    assert!(service.status().available);
    let png = service
        .render(1, Arc::clone(&bytes), "", 0, 200)
        .expect("the next request succeeds");
    assert_eq!(image::load_from_memory(&png).unwrap().width(), 200);
    let pid = service.worker_pid().expect("another worker runs");
    assert_ne!(pid, killed);
    // The page that fell once is drawn by the new worker.
    service
        .render(1, bytes, "", 0, 300)
        .expect("the same page again");
    assert_eq!(service.worker_pid(), Some(pid));
}

/// An encrypted document after a restart: the new worker is sent the
/// document and its password again, and the password is in no command
/// line.
#[test]
fn an_encrypted_document_is_drawn_again_after_a_restart() {
    let Some(service) = service() else { return };
    let bytes = fixture("encrypted-aes256.pdf");
    let error = service
        .render(1, Arc::clone(&bytes), "wrong", 0, 100)
        .unwrap_err();
    assert!(error.contains("ne peut pas ouvrir"), "{error}");
    service
        .render(2, Arc::clone(&bytes), "owner", 0, 100)
        .expect("render with the password");
    let pid = service.worker_pid().expect("a worker runs");
    if let Some(line) = command_line(pid) {
        assert!(line.trim_end().ends_with(worker::ARGUMENT), "{line}");
        assert!(!line.contains("owner"), "{line}");
    }
    kill(pid);
    assert!(gone_within(pid, PATIENCE));
    service
        .render(2, bytes, "owner", 0, 100)
        .expect("render again, by another worker");
    let again = service.worker_pid().expect("another worker runs");
    assert_ne!(again, pid);
}

/// The command line of process `pid`, where the system gives it cheaply.
fn command_line(pid: u32) -> Option<String> {
    if cfg!(target_os = "linux") {
        let raw = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
        return Some(String::from_utf8_lossy(&raw).replace('\0', " "));
    }
    if cfg!(windows) {
        let output = Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                &format!("(Get-CimInstance Win32_Process -Filter 'ProcessId={pid}').CommandLine"),
            ])
            .output()
            .ok()?;
        let line = String::from_utf8_lossy(&output.stdout).trim().to_string();
        return (!line.is_empty()).then_some(line);
    }
    None
}

/// A page whose image would pass the ceilings of the protocol is refused
/// by the worker, in words, and the worker stays.
#[test]
fn a_page_too_tall_is_refused_and_the_worker_stays() {
    let Some(service) = service() else { return };
    let pid = service.worker_pid();
    let ribbon = pdf("0 0 10 14400", "0 0 10 14400 re f");
    let error = service
        .render(1, Arc::clone(&ribbon), "", 0, 16)
        .unwrap_err();
    assert!(error.contains("la page 1 est trop haute"), "{error}");
    assert_eq!(service.worker_pid(), pid);
    // Not so tall a page, at the ceiling of the height or under it.
    let strip = pdf("0 0 10 1000", "0 0 10 1000 re f");
    let png = service.render(2, strip, "", 0, 100).expect("render");
    let image = image::load_from_memory(&png).unwrap();
    assert_eq!((image.width(), image.height()), (100, 10_000));
    assert_eq!(service.worker_pid(), pid);
}

/// A worker that holds more than its ceiling is stopped by the window, for
/// real: a page 4096 pixels wide takes more than the 64 Mio it is given
/// here. The request fails and says why, and the next one, a small image,
/// is drawn by another worker.
#[test]
fn a_worker_over_its_ceiling_is_stopped_and_the_next_page_is_drawn() {
    if cfg!(not(any(windows, target_os = "linux"))) {
        eprintln!("the memory of a process is not read on this system: skipped");
        return;
    }
    let tight = Limits {
        memory: 64 << 20,
        watch: Duration::from_millis(5),
        ..Limits::default()
    };
    let tight =
        RenderService::start_with(Box::new(Executable::new(FYP_APP)), &[pdfium_dir()], tight);
    if !tight.status().available {
        eprintln!("PDFium not fetched: skipped ({})", tight.status().detail);
        return;
    }
    let bytes = fixture("mixed12.pdf");
    tight
        .render(1, Arc::clone(&bytes), "", 0, 160)
        .expect("a thumbnail fits");
    let first = tight.worker_pid().expect("a worker runs");
    let error = tight
        .render(1, Arc::clone(&bytes), "", 0, 4096)
        .unwrap_err();
    assert_eq!(
        error,
        "le moteur de rendu a dépassé son plafond de mémoire (64 Mio) ; il a été arrêté et sera relancé"
    );
    assert!(gone_within(first, PATIENCE));
    assert_eq!(tight.worker_pid(), None);
    let png = tight
        .render(1, Arc::clone(&bytes), "", 1, 300)
        .expect("the next request succeeds");
    assert_eq!(image::load_from_memory(&png).unwrap().width(), 300);
    assert_ne!(tight.worker_pid(), Some(first));
    drop(tight);
    // With the ceiling of the application, the same page is drawn.
    let Some(service) = service() else { return };
    service
        .render(1, bytes, "", 0, 4096)
        .expect("the page fits the default ceiling");
}

/// The window reads what its worker holds through the system, without
/// PDFium and before the worker is told anything.
#[test]
fn the_memory_of_a_worker_is_read() {
    let mut link = Executable::new(FYP_APP).launch().expect("start the worker");
    let started = Instant::now();
    let mut held = link.process.memory();
    // Linux tells nothing of a process between `fork` and `exec`.
    while held.is_none_or(|held| held == 0) && started.elapsed() < Duration::from_secs(5) {
        thread::sleep(Duration::from_millis(10));
        held = link.process.memory();
    }
    if cfg!(any(windows, target_os = "linux")) {
        let held = held.expect("the system tells");
        assert!(held > 0 && held < 256 << 20, "{held} bytes");
    } else {
        assert_eq!(held, None);
    }
    link.process.kill();
    assert!(link.process.exited());
}

/// The stand-in of a window that dies: run by the test below, in a process
/// of its own, it starts a worker on a page that takes long to draw, says
/// which process it is, and waits to be killed.
#[test]
fn stand_in_for_a_window_that_dies() {
    if std::env::var_os(STAND_IN).is_none() {
        return;
    }
    let Some(service) = service() else {
        println!("worker none");
        return;
    };
    let service = Arc::new(service);
    let bytes = endless_page();
    let request = {
        let service = Arc::clone(&service);
        thread::spawn(move || service.render(1, bytes, "", 0, 4096))
    };
    let started = Instant::now();
    while !service.is_drawing() && started.elapsed() < PATIENCE {
        thread::yield_now();
    }
    println!("worker {}", service.worker_pid().unwrap_or(0));
    let _ = request.join();
    println!("drawn");
    thread::sleep(Duration::from_secs(600));
}

/// The variable that makes [`stand_in_for_a_window_that_dies`] run.
const STAND_IN: &str = "FYP_TEST_WINDOW_STAND_IN";

/// A page that takes PDFium much longer than the tests wait: a form of a
/// thousand filled curves, drawn by a form a thousand times.
fn endless_page() -> Arc<Vec<u8>> {
    let mut curves = String::new();
    for i in 0..1000_u32 {
        let (x, y) = (i * 7 % 590, i * 13 % 840);
        writeln!(
            curves,
            "{} {} m {} {} {} {} {} {} c f",
            x,
            y,
            x + 200,
            y + 50,
            x + 50,
            y + 300,
            x + 5,
            y + 5
        )
        .unwrap();
    }
    let calls = "/F1 Do\n".repeat(1000);
    let form = |resources: &str, content: &str| {
        format!(
            "<< /Type /XObject /Subtype /Form /BBox [0 0 595 842] {resources}/Length {} >>\nstream\n{content}\nendstream",
            content.len()
        )
    };
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 4 0 R /Resources << /XObject << /F1 5 0 R >> >> >>".to_string(),
        "<< /Length 7 >>\nstream\n/F1 Do\n\nendstream".to_string(),
        form("/Resources << /XObject << /F1 6 0 R >> >> ", &calls),
        form("", &curves),
    ];
    let mut file = String::from("%PDF-1.7\n");
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(file.len());
        write!(file, "{} 0 obj\n{object}\nendobj\n", index + 1).unwrap();
    }
    let xref = file.len();
    write!(file, "xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).unwrap();
    for offset in offsets {
        writeln!(file, "{offset:010} 00000 n ").unwrap();
    }
    write!(
        file,
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        objects.len() + 1
    )
    .unwrap();
    Arc::new(file.into_bytes())
}

/// On Windows the worker never outlives the window, even one that is
/// killed while its worker draws: the job the worker is in ends it at
/// once, where the end of its input would only be read after the drawing.
#[cfg(windows)]
#[test]
fn a_worker_that_draws_ends_with_the_window_that_is_killed() {
    use std::io::{BufRead, BufReader};
    let mut window = Command::new(std::env::current_exe().unwrap())
        .args(["stand_in_for_a_window_that_dies", "--exact", "--nocapture"])
        .env(STAND_IN, "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("start the stand-in");
    let mut lines = BufReader::new(window.stdout.take().unwrap()).lines();
    let worker = lines
        .by_ref()
        .map_while(Result::ok)
        .find_map(|line| line.strip_prefix("worker ").map(str::to_string))
        .expect("the stand-in says its worker");
    if worker == "none" {
        eprintln!("PDFium not fetched: skipped");
        let _ = window.wait();
        return;
    }
    let worker: u32 = worker.parse().unwrap();
    assert!(alive(worker), "the worker draws");
    kill(window.id());
    let _ = window.wait();
    let ended = gone_within(worker, Duration::from_secs(3));
    if !ended {
        kill(worker);
    }
    assert!(ended, "the worker ended with its window");
    // It was still drawing: the job ended it, not the end of the page.
    assert!(!lines.map_while(Result::ok).any(|line| line == "drawn"));
}

/// Dropping the service, as closing the application does, leaves no
/// worker behind.
#[test]
fn closing_the_service_leaves_no_worker() {
    let Some(service) = service() else { return };
    service
        .render(1, fixture("mixed12.pdf"), "", 0, 100)
        .expect("render");
    let pid = service.worker_pid().expect("a worker runs");
    assert!(alive(pid));
    service.shutdown();
    assert_eq!(service.worker_pid(), None);
    assert!(!alive(pid), "the worker was waited for");
    assert!(service
        .render(1, fixture("mixed12.pdf"), "", 0, 100)
        .is_err());
}

/// The worker ends when its standard input closes, whenever that is:
/// before the handshake, after it, after a document.
#[test]
fn the_worker_stops_when_its_input_closes() {
    let mut worker = ByHand::start();
    worker.close_input();
    let status = worker.exit_within(PATIENCE).expect("ends before a hello");
    assert_eq!(status.code(), Some(0));

    let mut worker = ByHand::start();
    worker.tell(&Request::Hello {
        version: VERSION,
        candidates: vec![pdfium_dir()],
    });
    let Ok(Reply::Ready {
        version: VERSION,
        available,
        ..
    }) = worker.hear()
    else {
        panic!("no answer to the handshake");
    };
    if !available {
        eprintln!("PDFium not fetched: the worker stops by itself");
        let status = worker.exit_within(PATIENCE).expect("ends without PDFium");
        assert_eq!(status.code(), Some(0));
        return;
    }
    worker.tell(&Request::Document {
        sequence: 1,
        id: 1,
        password: String::new(),
        bytes: fixture("mixed12.pdf").to_vec(),
    });
    assert_eq!(worker.hear(), Ok(Reply::Opened { sequence: 1 }));
    // It waits for requests as long as its input is open.
    thread::sleep(Duration::from_millis(300));
    assert!(worker.child.try_wait().unwrap().is_none());
    worker.close_input();
    let status = worker
        .exit_within(PATIENCE)
        .expect("ends when its input closes");
    assert_eq!(status.code(), Some(0));
    assert_eq!(worker.hear(), Err(protocol::ProtocolError::Closed));
}

/// A window that speaks another version is refused, and the worker ends.
#[test]
fn the_worker_refuses_another_version() {
    let mut worker = ByHand::start();
    worker.tell(&Request::Hello {
        version: VERSION + 1,
        candidates: vec![pdfium_dir()],
    });
    assert_eq!(worker.hear(), Ok(Reply::Refused { version: VERSION }));
    let status = worker.exit_within(PATIENCE).expect("ends once refused");
    assert_eq!(status.code(), Some(2));
}

/// What is not the protocol ends the worker, which answers nothing: a
/// request before the handshake, bytes that are no frame, and after the
/// handshake a frame cut short or a second hello.
#[test]
fn the_worker_stops_on_what_it_does_not_understand() {
    let draw = Request::Draw {
        sequence: 1,
        id: 1,
        page: 0,
        width: 100,
    };
    let mut worker = ByHand::start();
    worker.tell(&draw);
    assert_eq!(worker.hear(), Err(protocol::ProtocolError::Closed));
    assert_eq!(worker.exit_within(PATIENCE).and_then(|s| s.code()), Some(2));

    let mut worker = ByHand::start();
    let _ = worker
        .input
        .as_mut()
        .unwrap()
        .write_all(b"%PDF-1.7\nnot a frame at all, at some length\n");
    assert_eq!(worker.hear(), Err(protocol::ProtocolError::Closed));
    assert_eq!(worker.exit_within(PATIENCE).and_then(|s| s.code()), Some(2));

    let hello = Request::Hello {
        version: VERSION,
        candidates: vec![pdfium_dir()],
    };
    let mut worker = ByHand::start();
    worker.tell(&hello);
    if !matches!(
        worker.hear(),
        Ok(Reply::Ready {
            available: true,
            ..
        })
    ) {
        eprintln!("PDFium not fetched: only the refusals before the handshake were checked");
        return;
    }
    // A page of a document it was never sent: an answer, and it goes on.
    worker.tell(&draw);
    assert!(matches!(
        worker.hear(),
        Ok(Reply::Failed { sequence: 1, message }) if message == "document non chargé"
    ));
    // A width it is never asked for.
    worker.tell(&Request::Document {
        sequence: 2,
        id: 1,
        password: String::new(),
        bytes: fixture("mixed12.pdf").to_vec(),
    });
    assert_eq!(worker.hear(), Ok(Reply::Opened { sequence: 2 }));
    for width in [0, 15, 4097, u32::MAX] {
        worker.tell(&Request::Draw {
            sequence: 3,
            id: 1,
            page: 0,
            width,
        });
        assert!(
            matches!(worker.hear(), Ok(Reply::Failed { sequence: 3, message }) if message.contains("hors limites")),
            "{width}"
        );
    }
    worker.tell(&Request::Draw {
        sequence: 4,
        id: 1,
        page: u32::MAX,
        width: 100,
    });
    assert!(
        matches!(worker.hear(), Ok(Reply::Failed { sequence: 4, message }) if message.contains("hors de portée"))
    );
    // A second hello is not a request.
    worker.tell(&hello);
    assert_eq!(worker.hear(), Err(protocol::ProtocolError::Closed));
    assert_eq!(worker.exit_within(PATIENCE).and_then(|s| s.code()), Some(2));

    // A frame cut short: the worker waits for the rest, and ends with its
    // input.
    let mut worker = ByHand::start();
    worker.tell(&hello);
    assert!(matches!(worker.hear(), Ok(Reply::Ready { .. })));
    let mut cut = Vec::new();
    draw.write_to(&mut cut).unwrap();
    let _ = worker.input.as_mut().unwrap().write_all(&cut[..10]);
    worker.close_input();
    assert_eq!(worker.hear(), Err(protocol::ProtocolError::Closed));
    assert_eq!(worker.exit_within(PATIENCE).and_then(|s| s.code()), Some(2));
}
