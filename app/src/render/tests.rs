//! The service of the window against workers that are threads of the test:
//! honest, dying, silent or lying, without PDFium. The real worker is run
//! by `tests/render_worker.rs`.

use std::io::PipeWriter;
use std::sync::atomic::AtomicU32;

use super::protocol::Request;
use super::*;

/// What a fake worker does with a request.
enum Act {
    Reply(Reply),
    /// Bytes written as they are.
    Raw(Vec<u8>),
    Silence,
    Die,
}

type Script = dyn Fn(u32, &Request) -> Act + Send + Sync;

/// What the tests see of the fake workers started.
#[derive(Default)]
struct Seen {
    launches: AtomicU32,
    kills: AtomicU32,
    /// Each request received: the launch that received it, and its kind.
    requests: Mutex<Vec<(u32, &'static str)>>,
    /// The password of each document received.
    passwords: Mutex<Vec<String>>,
}

impl Seen {
    fn launches(&self) -> u32 {
        self.launches.load(Ordering::SeqCst)
    }

    fn kills(&self) -> u32 {
        self.kills.load(Ordering::SeqCst)
    }

    fn count(&self, kind: &str) -> usize {
        let requests = self.requests.lock().unwrap();
        requests.iter().filter(|(_, k)| *k == kind).count()
    }
}

struct Fake {
    script: Arc<Script>,
    seen: Arc<Seen>,
}

struct FakeProcess {
    id: u32,
    exited: Arc<AtomicBool>,
    output: Arc<Mutex<Option<PipeWriter>>>,
    seen: Arc<Seen>,
}

impl Process for FakeProcess {
    fn id(&self) -> u32 {
        self.id
    }

    fn exited(&mut self) -> bool {
        self.exited.load(Ordering::SeqCst)
    }

    fn kill(&mut self) {
        if !self.exited.swap(true, Ordering::SeqCst) {
            self.seen.kills.fetch_add(1, Ordering::SeqCst);
        }
        self.output.lock().unwrap().take();
    }
}

impl Launch for Fake {
    fn launch(&self) -> io::Result<Link> {
        let launch = self.seen.launches.fetch_add(1, Ordering::SeqCst) + 1;
        let (mut requests, input) = io::pipe()?;
        let (output, replies) = io::pipe()?;
        let replies = Arc::new(Mutex::new(Some(replies)));
        let exited = Arc::new(AtomicBool::new(false));
        let process = FakeProcess {
            id: 1000 + launch,
            exited: Arc::clone(&exited),
            output: Arc::clone(&replies),
            seen: Arc::clone(&self.seen),
        };
        let script = Arc::clone(&self.script);
        let seen = Arc::clone(&self.seen);
        thread::spawn(move || {
            while let Ok(Some(request)) = protocol::read_request(&mut requests) {
                let kind = match &request {
                    Request::Hello { .. } => "hello",
                    Request::Document { password, .. } => {
                        seen.passwords.lock().unwrap().push(password.clone());
                        "document"
                    }
                    Request::Draw { .. } => "draw",
                };
                seen.requests.lock().unwrap().push((launch, kind));
                let act = script(launch, &request);
                let mut replies = replies.lock().unwrap();
                match act {
                    Act::Reply(reply) => {
                        if let Some(out) = replies.as_mut() {
                            let _ = reply.write_to(out);
                        }
                    }
                    Act::Raw(bytes) => {
                        if let Some(out) = replies.as_mut() {
                            let _ = out.write_all(&bytes);
                        }
                    }
                    Act::Silence => {}
                    Act::Die => {
                        replies.take();
                        break;
                    }
                }
            }
            // Its input closed, or it died: either way it is gone.
            replies.lock().unwrap().take();
            exited.store(true, Ordering::SeqCst);
        });
        Ok(Link {
            input: Box::new(input),
            output: Box::new(output),
            process: Box::new(process),
        })
    }
}

/// What an honest worker answers: ready, opened, and an image twice as
/// tall as wide.
fn honest(request: &Request) -> Act {
    Act::Reply(match request {
        Request::Hello { .. } => Reply::Ready {
            version: VERSION,
            available: true,
            detail: "faux moteur".into(),
        },
        Request::Document { sequence, .. } => Reply::Opened {
            sequence: *sequence,
        },
        Request::Draw {
            sequence, width, ..
        } => Reply::Bitmap {
            sequence: *sequence,
            width: *width,
            height: width * 2,
            pixels: vec![0x80; (width * width * 2 * 4) as usize],
        },
    })
}

fn limits() -> Limits {
    Limits {
        handshake: Duration::from_secs(5),
        request: Duration::from_secs(5),
        restarts: 8,
        restart_window: Duration::from_secs(60),
        farewell: Duration::from_secs(5),
    }
}

fn service_with(
    limits: Limits,
    script: impl Fn(u32, &Request) -> Act + Send + Sync + 'static,
) -> (RenderService, Arc<Seen>) {
    let seen = Arc::new(Seen::default());
    let fake = Fake {
        script: Arc::new(script),
        seen: Arc::clone(&seen),
    };
    (RenderService::start_with(Box::new(fake), &[], limits), seen)
}

fn service(
    script: impl Fn(u32, &Request) -> Act + Send + Sync + 'static,
) -> (RenderService, Arc<Seen>) {
    service_with(limits(), script)
}

fn bytes() -> Arc<Vec<u8>> {
    Arc::new(b"%PDF-1.7 for a fake".to_vec())
}

/// Whether `request` asks for page `page` (0-based).
fn draws(request: &Request, page: u32) -> bool {
    matches!(request, Request::Draw { page: asked, .. } if *asked == page)
}

#[test]
fn an_honest_worker_gives_a_png_of_the_width_asked() {
    let (service, seen) = service(|_, request| honest(request));
    assert_eq!(
        service.status(),
        Status {
            available: true,
            detail: "faux moteur".into()
        }
    );
    assert_eq!(service.worker_pid(), Some(1001));
    let png = service.render(1, bytes(), "", 0, 20).expect("render");
    let image = image::load_from_memory(&png).expect("decode");
    assert_eq!((image.width(), image.height()), (20, 40));
    assert_eq!(image.to_rgba8().get_pixel(3, 3).0, [0x80; 4]);
    // The same document is sent once, another one again.
    service.render(1, bytes(), "", 1, 20).expect("render");
    assert_eq!(seen.count("document"), 1);
    service.render(2, bytes(), "secret", 0, 20).expect("render");
    assert_eq!(seen.count("document"), 2);
    assert_eq!(*seen.passwords.lock().unwrap(), ["", "secret"]);
    assert_eq!(seen.launches(), 1);
    // The width is brought within what the worker draws.
    let png = service.render(2, bytes(), "secret", 0, 1).expect("render");
    assert_eq!(image::load_from_memory(&png).unwrap().width(), MIN_WIDTH);
}

/// The worker dies while it draws: the request fails in words, the next
/// one starts another worker, which is sent the document, and its password,
/// again.
#[test]
fn a_worker_that_dies_fails_the_request_and_is_started_again() {
    let (service, seen) = service(|launch, request| {
        if launch == 1 && draws(request, 3) {
            Act::Die
        } else {
            honest(request)
        }
    });
    service.render(1, bytes(), "owner", 0, 16).expect("render");
    let error = service.render(1, bytes(), "owner", 3, 16).unwrap_err();
    assert_eq!(error, "le moteur de rendu s'est arrêté ; il sera relancé");
    assert_eq!(seen.launches(), 1, "started again on demand only");
    assert_eq!(service.worker_pid(), None);
    service.render(1, bytes(), "owner", 0, 16).expect("render");
    assert_eq!(seen.launches(), 2);
    assert_eq!(seen.count("document"), 2);
    assert_eq!(*seen.passwords.lock().unwrap(), ["owner", "owner"]);
    assert!(service.status().available);
    // The page that fell once is tried once more, by the new worker.
    service.render(1, bytes(), "owner", 3, 16).expect("render");
    assert_eq!(seen.launches(), 2);
}

/// A page that brings the worker down twice is refused from then on,
/// without a worker being started for it; the other pages are drawn, and
/// another document starts from nothing.
#[test]
fn a_page_that_brings_the_worker_down_twice_is_refused() {
    let (service, seen) = service(|_, request| {
        if draws(request, 4) {
            Act::Die
        } else {
            honest(request)
        }
    });
    for _ in 0..2 {
        let error = service.render(1, bytes(), "", 4, 16).unwrap_err();
        assert!(error.contains("s'est arrêté"), "{error}");
    }
    assert_eq!(seen.launches(), 2);
    assert_eq!(service.worker_pid(), None);
    for _ in 0..3 {
        let error = service.render(1, bytes(), "", 4, 16).unwrap_err();
        assert_eq!(
            error,
            "la page 5 a arrêté le moteur de rendu deux fois ; elle n'est plus dessinée"
        );
    }
    assert_eq!(seen.launches(), 2, "no worker started for a refused page");
    assert_eq!(service.worker_pid(), None);
    service.render(1, bytes(), "", 5, 16).expect("another page");
    assert_eq!(seen.launches(), 3);
    assert!(service.render(1, bytes(), "", 4, 16).is_err());
    assert_eq!(seen.launches(), 3);
    // Another document: its page 5 is tried again.
    let error = service.render(2, bytes(), "", 4, 16).unwrap_err();
    assert!(error.contains("s'est arrêté"), "{error}");
    assert_eq!(seen.launches(), 3, "the worker was running");
    assert!(service.status().available);
}

/// A document that brings the worker down when it is loaded is refused
/// after two falls, whatever the page: not two falls per page.
#[test]
fn a_document_that_brings_the_worker_down_at_opening_is_refused() {
    let (service, seen) = service(|_, request| match request {
        Request::Document { id: 1, .. } => Act::Die,
        _ => honest(request),
    });
    assert!(service.render(1, bytes(), "", 0, 16).is_err());
    assert!(service.render(1, bytes(), "", 1, 16).is_err());
    assert_eq!(seen.launches(), 2);
    for page in 0..5 {
        let error = service.render(1, bytes(), "", page, 16).unwrap_err();
        assert!(error.contains("deux fois à l'ouverture"), "{error}");
    }
    assert_eq!(seen.launches(), 2);
    service
        .render(2, bytes(), "", 0, 16)
        .expect("another document");
    assert_eq!(seen.launches(), 3);
}

/// A worker that does not answer in time is killed; the request fails and
/// the next one is served by another worker.
#[test]
fn a_silent_worker_is_killed_when_the_delay_is_over() {
    let patient = Limits {
        request: Duration::from_millis(300),
        ..limits()
    };
    let (service, seen) = service_with(patient, |launch, request| {
        if launch == 1 && draws(request, 0) {
            Act::Silence
        } else {
            honest(request)
        }
    });
    let started = Instant::now();
    let error = service.render(1, bytes(), "", 0, 16).unwrap_err();
    assert!(started.elapsed() >= Duration::from_millis(300));
    assert!(started.elapsed() < Duration::from_secs(4));
    assert_eq!(
        error,
        "le moteur de rendu n'a pas répondu en 1 s ; il a été arrêté et sera relancé"
    );
    assert_eq!(seen.kills(), 1);
    assert_eq!(service.worker_pid(), None);
    service.render(1, bytes(), "", 0, 16).expect("render");
    assert_eq!(seen.launches(), 2);
}

/// A worker that answers anything but the reply to the request is stopped
/// like one that fell: each lie costs the request, never the window.
#[test]
fn a_lying_worker_is_stopped() {
    fn reply_to(request: &Request, lie: u32) -> Act {
        let Request::Draw {
            sequence, width, ..
        } = request
        else {
            return honest(request);
        };
        let (sequence, width) = (*sequence, *width);
        let image = |sequence, width: u32, height: u32, pixels: usize| {
            let mut frame = vec![0x84];
            frame.extend_from_slice(&(16 + pixels as u32).to_le_bytes());
            frame.extend_from_slice(&u64::to_le_bytes(sequence));
            frame.extend_from_slice(&width.to_le_bytes());
            frame.extend_from_slice(&height.to_le_bytes());
            frame.resize(frame.len() + pixels, 0);
            Act::Raw(frame)
        };
        match lie {
            // Another width than the one asked, well formed.
            1 => image(sequence, width + 1, 1, (width as usize + 1) * 4),
            // The reply to another request.
            2 => image(sequence + 1, width, 1, width as usize * 4),
            // Dimensions that are not the length.
            3 => image(sequence, width, 2, width as usize * 4),
            // A length no image has.
            4 => Act::Raw(vec![0x84, 0xff, 0xff, 0xff, 0xff]),
            // An image as tall as the frame allows, and nothing after.
            5 => {
                let mut frame = vec![0x84];
                frame.extend_from_slice(&(16 + 16 * protocol::MAX_HEIGHT * 4).to_le_bytes());
                frame.extend_from_slice(&u64::to_le_bytes(sequence));
                frame.extend_from_slice(&16_u32.to_le_bytes());
                frame.extend_from_slice(&protocol::MAX_HEIGHT.to_le_bytes());
                Act::Raw(frame)
            }
            // Not a frame at all.
            6 => Act::Raw(b"Bonjour, je suis PDFium.\n".to_vec()),
            // An answer to a question nobody asked.
            7 => Act::Reply(Reply::Opened { sequence }),
            8 => Act::Reply(Reply::Ready {
                version: VERSION,
                available: true,
                detail: "encore".into(),
            }),
            _ => honest(request),
        }
    }
    let patient = Limits {
        request: Duration::from_millis(500),
        restarts: 100,
        ..limits()
    };
    let (service, seen) = service_with(patient, |launch, request| reply_to(request, launch));
    for lie in 1..=8_u32 {
        // One page per lie: none is refused for having fallen twice.
        let error = service
            .render(1, bytes(), "", lie as usize, 16)
            .unwrap_err();
        if lie == 5 {
            // Nothing after the dimensions: silence, as far as the window
            // can tell.
            assert!(error.contains("n'a pas répondu"), "lie {lie}: {error}");
        } else {
            assert!(error.contains("réponse inattendue"), "lie {lie}: {error}");
        }
        assert!(error.contains("sera relancé"), "lie {lie}: {error}");
        assert_eq!(service.worker_pid(), None, "lie {lie}");
        assert_eq!(seen.launches(), lie);
    }
    assert!(service.status().available);
    let png = service.render(1, bytes(), "", 0, 16).expect("honest again");
    assert_eq!(image::load_from_memory(&png).unwrap().width(), 16);
}

/// A reply the worker sends without being asked is not taken for the
/// answer to the next request.
#[test]
fn an_unasked_reply_is_not_taken_for_the_next_answer() {
    let (service, _seen) = service(|launch, request| match request {
        Request::Draw {
            sequence, width, ..
        } if launch == 1 => {
            let mut twice = Vec::new();
            for sequence in [*sequence, *sequence + 1] {
                Reply::Bitmap {
                    sequence,
                    width: *width,
                    height: 1,
                    pixels: vec![0; *width as usize * 4],
                }
                .write_to(&mut twice)
                .unwrap();
            }
            Act::Raw(twice)
        }
        _ => honest(request),
    });
    service
        .render(1, bytes(), "", 0, 16)
        .expect("the first one");
    // The second image was written before this request: its sequence is
    // not the one of this request.
    let error = service.render(1, bytes(), "", 1, 16).unwrap_err();
    assert!(error.contains("réponse à une autre demande"), "{error}");
}

/// Too many restarts in a short time: rendering is off for the session,
/// the status says so, and no worker is started any more.
#[test]
fn too_many_restarts_turn_rendering_off() {
    let few = Limits {
        restarts: 3,
        ..limits()
    };
    let (service, seen) = service_with(few, |_, request| match request {
        Request::Draw { .. } => Act::Die,
        _ => honest(request),
    });
    // One page each: none reaches its two falls.
    for page in 0..4 {
        let error = service.render(1, bytes(), "", page, 16).unwrap_err();
        assert!(error.contains("s'est arrêté ;"), "{error}");
        assert!(service.status().available);
    }
    assert_eq!(seen.launches(), 4, "the first start and three restarts");
    let error = service.render(1, bytes(), "", 4, 16).unwrap_err();
    assert!(error.contains("trop souvent (3 relances"), "{error}");
    let status = service.status();
    assert!(!status.available);
    assert_eq!(status.detail, error);
    assert_eq!(service.render(2, bytes(), "", 0, 16).unwrap_err(), error);
    assert_eq!(seen.launches(), 4);
}

/// Restarts are counted over a window: old ones are forgotten.
#[test]
fn restarts_are_counted_over_a_window() {
    let short = Limits {
        restarts: 1,
        restart_window: Duration::from_millis(200),
        ..limits()
    };
    let (service, seen) = service_with(short, |_, request| match request {
        Request::Draw { .. } => Act::Die,
        _ => honest(request),
    });
    for page in 0..3 {
        assert!(service.render(1, bytes(), "", page, 16).is_err());
        thread::sleep(Duration::from_millis(250));
    }
    assert!(service.status().available);
    assert_eq!(seen.launches(), 3);
}

/// A worker killed between two requests is replaced without a request
/// failing: none of them is to blame.
#[test]
fn a_worker_found_dead_is_replaced_before_the_request() {
    let (service, seen) = service(|_, request| honest(request));
    service.render(1, bytes(), "", 0, 16).expect("render");
    service.process().as_mut().unwrap().kill();
    service.render(1, bytes(), "", 0, 16).expect("render");
    assert_eq!(seen.launches(), 2);
    assert_eq!(seen.count("document"), 2);
    assert_eq!(service.worker_pid(), Some(1002));
}

/// A document the worker cannot open is refused once, with its reason, not
/// sent again for each page; the worker stays.
#[test]
fn a_document_the_worker_refuses_is_sent_once() {
    let (service, seen) = service(|_, request| match request {
        Request::Document {
            sequence, id: 1, ..
        } => Act::Reply(Reply::Failed {
            sequence: *sequence,
            message: "PDFium ne peut pas ouvrir ce fichier".into(),
        }),
        Request::Draw {
            sequence, page: 9, ..
        } => Act::Reply(Reply::Failed {
            sequence: *sequence,
            message: "page 10 hors de portée".into(),
        }),
        _ => honest(request),
    });
    for page in 0..3 {
        let error = service.render(1, bytes(), "", page, 16).unwrap_err();
        assert_eq!(error, "PDFium ne peut pas ouvrir ce fichier");
    }
    assert_eq!(seen.count("document"), 1);
    // A page that cannot be drawn is the worker's answer, not its fall.
    let error = service.render(2, bytes(), "", 9, 16).unwrap_err();
    assert_eq!(error, "page 10 hors de portée");
    service.render(2, bytes(), "", 0, 16).expect("render");
    assert_eq!((seen.launches(), seen.kills()), (1, 0));
    // A page number the protocol cannot carry never reaches the worker.
    if usize::BITS > 32 {
        let error = service
            .render(2, bytes(), "", u32::MAX as usize + 1, 16)
            .unwrap_err();
        assert!(error.contains("hors de portée"), "{error}");
        assert_eq!(seen.count("draw"), 2);
    }
}

/// The handshake: a worker of another version and one without a library
/// leave a service that says why, never asks them for a page and starts no
/// other.
#[test]
fn a_worker_that_refuses_the_handshake_leaves_rendering_off() {
    let hello = |answer: fn() -> Act| {
        let (service, seen) = service(move |_, _| answer());
        let status = service.status();
        assert!(!status.available);
        for _ in 0..3 {
            assert_eq!(
                service.render(1, bytes(), "", 0, 16).unwrap_err(),
                status.detail
            );
        }
        assert_eq!(service.worker_pid(), None);
        assert_eq!(seen.launches(), 1);
        assert_eq!(seen.count("document"), 0);
        status.detail
    };
    let detail = hello(|| Act::Reply(Reply::Refused { version: 7 }));
    assert_eq!(
        detail,
        "le moteur de rendu parle le protocole 7, la fenêtre le protocole 1"
    );
    let detail = hello(|| {
        Act::Reply(Reply::Ready {
            version: VERSION + 1,
            available: true,
            detail: "PDFium".into(),
        })
    });
    assert!(detail.contains("parle le protocole 2"), "{detail}");
    let detail = hello(|| {
        Act::Reply(Reply::Ready {
            version: VERSION,
            available: false,
            detail: "bibliothèque PDFium introuvable".into(),
        })
    });
    assert_eq!(detail, "bibliothèque PDFium introuvable");

    let service = RenderService::unavailable("pas de moteur ici");
    assert_eq!(service.status().detail, "pas de moteur ici");
    assert_eq!(
        service.render(1, bytes(), "", 0, 16).unwrap_err(),
        "pas de moteur ici"
    );
}

/// A first worker that dies, stays silent or answers anything else before
/// it shook hands is a worker that fell: the first page asked for starts
/// another one, and is drawn.
#[test]
fn a_first_start_that_fails_is_tried_again_at_the_next_request() {
    let first = |answer: fn() -> Act, why: &str| {
        let quick = Limits {
            handshake: Duration::from_millis(300),
            ..limits()
        };
        let (service, seen) = service_with(quick, move |launch, request| {
            if launch == 1 {
                answer()
            } else {
                honest(request)
            }
        });
        let status = service.status();
        assert!(status.available, "{why}");
        assert!(
            status.detail.contains("n'a pas démarré"),
            "{}",
            status.detail
        );
        assert!(status.detail.contains(why), "{}", status.detail);
        assert_eq!(service.worker_pid(), None);
        assert_eq!(seen.launches(), 1);
        service.render(1, bytes(), "", 0, 16).expect("render");
        assert_eq!(seen.launches(), 2);
        assert_eq!(
            service.status(),
            Status {
                available: true,
                detail: "faux moteur".into()
            }
        );
    };
    first(|| Act::Die, "arrêté pendant la poignée de main");
    first(|| Act::Silence, "pas de réponse à la poignée de main");
    first(|| Act::Raw(vec![0xff; 64]), "poignée de main inattendue");
    first(
        || Act::Reply(Reply::Opened { sequence: 0 }),
        "poignée de main sans réponse",
    );

    // A worker that never starts: each request tries again and says why,
    // until restarts are too many.
    let few = Limits {
        restarts: 3,
        ..limits()
    };
    let service = RenderService::start_with(Box::new(NoWorker), &[], few);
    for _ in 0..3 {
        assert!(service.status().available);
        let error = service.render(1, bytes(), "", 0, 16).unwrap_err();
        assert!(error.contains("n'a pas démarré"), "{error}");
    }
    let error = service.render(1, bytes(), "", 0, 16).unwrap_err();
    assert!(error.contains("trop souvent"), "{error}");
    assert!(!service.status().available);
}

/// Closing while a request starts a worker again: once the handshake is
/// over, the new worker is let go of and asked nothing, and closing does
/// not wait for a document to be loaded or a page to be drawn.
#[test]
fn shutting_down_during_a_restart_asks_the_new_worker_nothing() {
    struct Gated {
        fake: Fake,
        entered: Mutex<Sender<()>>,
        go: Mutex<Receiver<()>>,
    }
    impl Launch for Gated {
        fn launch(&self) -> io::Result<Link> {
            if self.fake.seen.launches() >= 1 {
                // The restart: held until closing has begun.
                self.entered.lock().unwrap().send(()).unwrap();
                self.go.lock().unwrap().recv().unwrap();
            }
            self.fake.launch()
        }
    }
    let seen = Arc::new(Seen::default());
    let (entered, has_entered) = mpsc::channel();
    let (go, may_go) = mpsc::channel();
    let gated = Gated {
        fake: Fake {
            script: Arc::new(|launch, request| match request {
                Request::Draw { .. } if launch == 1 => Act::Die,
                Request::Draw { .. } => Act::Silence,
                _ => honest(request),
            }),
            seen: Arc::clone(&seen),
        },
        entered: Mutex::new(entered),
        go: Mutex::new(may_go),
    };
    let slow = Limits {
        request: Duration::from_secs(60),
        ..limits()
    };
    let service = Arc::new(RenderService::start_with(Box::new(gated), &[], slow));
    assert!(service.render(1, bytes(), "", 0, 16).is_err());
    let request = {
        let service = Arc::clone(&service);
        thread::spawn(move || service.render(1, bytes(), "", 1, 16))
    };
    has_entered.recv().unwrap();
    let started = Instant::now();
    let closing = {
        let service = Arc::clone(&service);
        thread::spawn(move || service.shutdown())
    };
    while !service.closing.load(Ordering::SeqCst) {
        thread::yield_now();
    }
    go.send(()).unwrap();
    let error = request.join().unwrap().unwrap_err();
    closing.join().unwrap();
    assert!(started.elapsed() < Duration::from_secs(20));
    assert!(error.contains("l'application se ferme"), "{error}");
    let second: Vec<_> = seen
        .requests
        .lock()
        .unwrap()
        .iter()
        .filter(|(launch, _)| *launch == 2)
        .map(|(_, kind)| *kind)
        .collect();
    assert_eq!(second, ["hello"]);
    assert_eq!(service.worker_pid(), None);
    assert_eq!(seen.launches(), 2);
}

/// Closing: the input of the worker is closed, it exits by itself and is
/// waited for; nothing is drawn afterwards, and nothing is started.
#[test]
fn shutting_down_closes_the_worker_and_waits_for_it() {
    let (service, seen) = service(|_, request| honest(request));
    service.render(1, bytes(), "", 0, 16).expect("render");
    service.shutdown();
    assert_eq!(service.worker_pid(), None);
    assert_eq!(seen.kills(), 0, "it left by itself when its input closed");
    let error = service.render(1, bytes(), "", 0, 16).unwrap_err();
    assert!(error.contains("l'application se ferme"), "{error}");
    assert!(!service.status().available);
    assert_eq!(seen.launches(), 1);
    service.shutdown();
}

/// A worker that does not leave when its input closes is killed once the
/// farewell is over.
#[test]
fn a_worker_that_lingers_is_killed() {
    struct Deaf(Arc<Seen>);
    impl Launch for Deaf {
        fn launch(&self) -> io::Result<Link> {
            let fake = Fake {
                script: Arc::new(|_, request| honest(request)),
                seen: Arc::clone(&self.0),
            };
            let link = fake.launch()?;
            // A process that never notices its input closed.
            struct Stubborn(Box<dyn Process>, Arc<AtomicBool>);
            impl Process for Stubborn {
                fn id(&self) -> u32 {
                    self.0.id()
                }
                fn exited(&mut self) -> bool {
                    self.1.load(Ordering::SeqCst)
                }
                fn kill(&mut self) {
                    self.1.store(true, Ordering::SeqCst);
                    self.0.kill();
                }
            }
            Ok(Link {
                process: Box::new(Stubborn(link.process, Arc::new(AtomicBool::new(false)))),
                ..link
            })
        }
    }
    let seen = Arc::new(Seen::default());
    let brief = Limits {
        farewell: Duration::from_millis(200),
        ..limits()
    };
    let service = RenderService::start_with(Box::new(Deaf(Arc::clone(&seen))), &[], brief);
    assert!(service.status().available);
    let started = Instant::now();
    drop(service);
    assert!(started.elapsed() >= Duration::from_millis(200));
    assert!(started.elapsed() < Duration::from_secs(4));
}

/// Closing while a page is being drawn does not wait for the delay of the
/// request: the worker is killed, and the request fails.
#[test]
fn shutting_down_does_not_wait_for_a_request_in_flight() {
    let slow = Limits {
        request: Duration::from_secs(60),
        ..limits()
    };
    let (service, seen) = service_with(slow, |_, request| match request {
        Request::Draw { .. } => Act::Silence,
        _ => honest(request),
    });
    let service = Arc::new(service);
    let request = {
        let service = Arc::clone(&service);
        thread::spawn(move || service.render(1, bytes(), "", 0, 16))
    };
    let started = Instant::now();
    while !service.is_drawing() {
        assert!(started.elapsed() < Duration::from_secs(10));
        thread::sleep(Duration::from_millis(5));
    }
    let started = Instant::now();
    service.shutdown();
    let outcome = request.join().unwrap();
    assert!(started.elapsed() < Duration::from_secs(10));
    assert!(outcome.is_err());
    assert_eq!(seen.kills(), 1);
    assert_eq!(service.worker_pid(), None);
    assert!(!service.is_drawing());
    assert_eq!(seen.launches(), 1);
}

/// Closing while a request finds its worker dead: no other is started.
#[test]
fn shutting_down_starts_no_worker_in_place_of_a_dead_one() {
    /// A process that, once `armed`, says it has ended only when `gate`
    /// opens: the request that asks holds its turn meanwhile.
    struct Waits {
        inner: Box<dyn Process>,
        armed: Arc<AtomicBool>,
        gate: Arc<AtomicBool>,
    }
    impl Process for Waits {
        fn id(&self) -> u32 {
            self.inner.id()
        }
        fn exited(&mut self) -> bool {
            if !self.armed.load(Ordering::SeqCst) {
                return self.inner.exited();
            }
            while !self.gate.load(Ordering::SeqCst) {
                thread::yield_now();
            }
            self.inner.kill();
            true
        }
        fn kill(&mut self) {
            self.inner.kill();
        }
    }
    struct Waiting {
        fake: Fake,
        armed: Arc<AtomicBool>,
        gate: Arc<AtomicBool>,
    }
    impl Launch for Waiting {
        fn launch(&self) -> io::Result<Link> {
            let link = self.fake.launch()?;
            Ok(Link {
                process: Box::new(Waits {
                    inner: link.process,
                    armed: Arc::clone(&self.armed),
                    gate: Arc::clone(&self.gate),
                }),
                ..link
            })
        }
    }
    let seen = Arc::new(Seen::default());
    let armed = Arc::new(AtomicBool::new(false));
    let gate = Arc::new(AtomicBool::new(false));
    let waiting = Waiting {
        fake: Fake {
            script: Arc::new(|_, request| honest(request)),
            seen: Arc::clone(&seen),
        },
        armed: Arc::clone(&armed),
        gate: Arc::clone(&gate),
    };
    let service = Arc::new(RenderService::start_with(Box::new(waiting), &[], limits()));
    service.render(1, bytes(), "", 0, 16).expect("render");
    armed.store(true, Ordering::SeqCst);
    let request = {
        let service = Arc::clone(&service);
        thread::spawn(move || service.render(1, bytes(), "", 1, 16))
    };
    // The request holds its turn, asking whether its worker has ended.
    while service.state.try_lock().is_ok() {
        thread::yield_now();
    }
    let closing = {
        let service = Arc::clone(&service);
        thread::spawn(move || service.shutdown())
    };
    while !service.closing.load(Ordering::SeqCst) {
        thread::yield_now();
    }
    gate.store(true, Ordering::SeqCst);
    let error = request.join().unwrap().unwrap_err();
    closing.join().unwrap();
    assert!(error.contains("l'application se ferme"), "{error}");
    assert_eq!(seen.launches(), 1, "no worker started while closing");
    assert_eq!(service.worker_pid(), None);
}

/// Requests from several threads at once pass one by one, each with its
/// own answer.
#[test]
fn concurrent_requests_each_get_their_own_page() {
    let (service, seen) = service(|_, request| match request {
        Request::Draw {
            sequence,
            page,
            width,
            ..
        } => Act::Reply(Reply::Bitmap {
            sequence: *sequence,
            width: *width,
            height: 1,
            pixels: vec![*page as u8; *width as usize * 4],
        }),
        _ => honest(request),
    });
    let service = Arc::new(service);
    let threads: Vec<_> = (0..8_usize)
        .map(|page| {
            let service = Arc::clone(&service);
            thread::spawn(move || {
                for _ in 0..10 {
                    let png = service.render(1, bytes(), "", page, 16).expect("render");
                    let image = image::load_from_memory(&png).expect("decode").to_rgba8();
                    assert_eq!(image.get_pixel(0, 0).0, [page as u8; 4]);
                }
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    assert_eq!((seen.launches(), seen.count("draw")), (1, 80));
}

/// A packaged build looks next to its executable, never in the checkout
/// it was built from; a development build looks there last.
#[test]
fn only_a_development_build_looks_in_the_checkout() {
    let exe_dir = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let checkout = Path::new(env!("CARGO_MANIFEST_DIR")).join("pdfium");
    let packaged = library_candidates(None);
    assert!(packaged.contains(&exe_dir));
    assert!(!packaged.contains(&checkout));
    let development = library_candidates(Some(checkout.clone()));
    assert_eq!(development.last(), Some(&checkout));
    assert_eq!(development[..development.len() - 1], packaged[..]);
}
