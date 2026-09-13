//! A run of the bench: the page set checked against its digests, both engines
//! on every document, one process at a time, then the images compared on
//! every thread. Everything a report needs ends up in a [`Run`].

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{ExtendedColorType, ImageEncoder, RgbImage};
use serde::Serialize;

use crate::compare::{self, Comparison};
use crate::engine::{self, DocumentRun, EngineInfo, EngineSpec, PageOutcome, Places};
use crate::metric::{self, Metric};
use crate::pageset::{self, FileState, PageEntry, PageSet};
use crate::protocol::{PageRequest, Request, PROTOCOL};
use crate::report::number;
use crate::system::{self, UtcTime};

/// What a run is asked to do.
#[derive(Debug, Clone)]
pub struct RunOptions {
    /// Root of the repository: the page files are relative to it, the
    /// engines run from it.
    pub root: PathBuf,
    /// Directory of the bench's executable, `{bin_dir}` in `engines.toml`.
    pub bin_dir: PathBuf,
    /// The engines file.
    pub engines_file: PathBuf,
    /// The page set file.
    pub pages_file: PathBuf,
    /// Engine A, the reference.
    pub a: String,
    /// Engine B, measured against A.
    pub b: String,
    /// Name of the metric.
    pub metric: String,
    /// How many times each engine draws and encodes each page.
    pub repeat: u32,
    /// Only the first pages of the set, for a quick look.
    pub limit: Option<usize>,
    /// Directory of the report of this run, created if needed.
    pub out: PathBuf,
    /// Silence after which an engine is stopped.
    pub timeout: Duration,
    /// Build the engines before running them.
    pub build: bool,
    /// Threads comparing images.
    pub jobs: usize,
    /// Tell the progress on standard error.
    pub verbose: bool,
    /// The command line, for the report.
    pub command_line: String,
}

/// Everything a run found.
#[derive(Debug, Clone, Serialize)]
pub struct Run {
    /// When it started.
    pub started: String,
    /// The processor, threads and system it ran on.
    pub machine: String,
    /// Commit of the checkout.
    pub revision: String,
    /// Compiler of the engines.
    pub rustc: String,
    /// The command line.
    pub command_line: String,
    /// The page set file, relative to the root.
    pub pages_file: String,
    /// SHA-256 of that file.
    pub pages_sha256: String,
    /// Pages in the set; the run may have measured only the first ones.
    pub set_pages: usize,
    /// Width of the images, in pixels.
    pub width: u32,
    /// Repetitions of each page.
    pub repeat: u32,
    /// Silence after which an engine was stopped, in seconds.
    pub timeout_s: u64,
    /// Name of the metric.
    pub metric: String,
    /// What its number means.
    pub metric_description: String,
    /// Engine A.
    pub a: EngineSide,
    /// Engine B.
    pub b: EngineSide,
    /// Every page, in the order of the set.
    pub pages: Vec<PageResult>,
    /// Wall time of the rendering, both engines, in seconds.
    pub render_s: f64,
    /// Wall time of the comparisons, in seconds.
    pub compare_s: f64,
}

/// One engine over the run.
#[derive(Debug, Clone, Serialize)]
pub struct EngineSide {
    /// Its name in `engines.toml`.
    pub name: String,
    /// Its description there.
    pub description: String,
    /// How it presented itself on the first document.
    pub info: Option<EngineInfo>,
    /// Other presentations on later documents, which should not happen.
    pub other_infos: Vec<EngineInfo>,
    /// Documents it was run on.
    pub documents: usize,
    /// Wall time of its processes, start to exit, in milliseconds.
    pub process_ms: f64,
}

impl EngineSide {
    fn new(name: &str, spec: &EngineSpec) -> EngineSide {
        EngineSide {
            name: name.to_string(),
            description: spec.description.clone(),
            info: None,
            other_infos: Vec::new(),
            documents: 0,
            process_ms: 0.0,
        }
    }

    fn absorb(&mut self, run: &DocumentRun) {
        self.documents += 1;
        self.process_ms += run.wall_ms;
        if let Some(info) = &run.engine {
            match &self.info {
                None => self.info = Some(info.clone()),
                Some(known) if known != info && !self.other_infos.contains(info) => {
                    self.other_infos.push(info.clone());
                }
                Some(_) => {}
            }
        }
    }
}

/// One engine on one page.
#[derive(Debug, Clone, Default, Serialize)]
pub struct EngineResult {
    /// Time to open the document, in milliseconds, when it opened.
    pub open_ms: Option<f64>,
    /// What became of the page; `None` when it was not run.
    pub outcome: Option<PageOutcome>,
    /// The end of the engine's standard error, when the page failed.
    pub stderr: Option<String>,
}

impl EngineResult {
    /// Whether the engine drew the page.
    pub fn rendered(&self) -> bool {
        matches!(self.outcome, Some(PageOutcome::Rendered { .. }))
    }

    /// The median time to draw the page, in milliseconds.
    pub fn render_ms(&self) -> Option<f64> {
        match &self.outcome {
            Some(PageOutcome::Rendered { render_ms, .. }) => crate::stats::median(render_ms),
            _ => None,
        }
    }

    /// The median time to encode the page, in milliseconds.
    pub fn encode_ms(&self) -> Option<f64> {
        match &self.outcome {
            Some(PageOutcome::Rendered { encode_ms, .. }) => crate::stats::median(encode_ms),
            _ => None,
        }
    }

    /// Why the page has no image, when it has none.
    pub fn error(&self) -> Option<&str> {
        match &self.outcome {
            Some(PageOutcome::Failed { error }) => Some(error),
            _ => None,
        }
    }
}

/// Where a page stands after the run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// B failed where A drew the page.
    BFailed,
    /// A failed where B drew the page.
    AFailed,
    /// Neither drew the page.
    BothFailed,
    /// Both drew the page; the images are compared.
    Compared,
    /// Not run: its file is missing or changed.
    NotMeasured,
}

/// A page of the run.
#[derive(Debug, Clone, Serialize)]
pub struct PageResult {
    /// Its position in the set, from 1.
    pub n: usize,
    /// The page as the set describes it.
    pub entry: PageEntry,
    /// What became of its file.
    pub file: FileState,
    /// Engine A on the page.
    pub a: EngineResult,
    /// Engine B on the page.
    pub b: EngineResult,
    /// The comparison of both images, when both engines drew the page.
    pub comparison: Option<Comparison>,
    /// Image of A, relative to the directory of the report.
    pub image_a: String,
    /// Image of B, relative to the directory of the report.
    pub image_b: String,
    /// Picture of the differences, relative to the directory of the report,
    /// when it was written.
    pub image_diff: Option<String>,
}

impl PageResult {
    /// Where the page stands.
    pub fn status(&self) -> Status {
        match (
            self.a.outcome.is_some(),
            self.a.rendered(),
            self.b.rendered(),
        ) {
            (false, _, _) => Status::NotMeasured,
            (true, true, true) => Status::Compared,
            (true, true, false) => Status::BFailed,
            (true, false, true) => Status::AFailed,
            (true, false, false) => Status::BothFailed,
        }
    }

    /// The distance between the images, when they were compared.
    pub fn distance(&self) -> Option<f64> {
        self.comparison.as_ref().map(|c| c.distance)
    }
}

/// The pages from worst to best: failures of B, then of A, then of both, then
/// the compared pages by decreasing distance, and last the pages not
/// measured.
pub fn worst_first(pages: &[PageResult]) -> Vec<&PageResult> {
    let mut sorted: Vec<&PageResult> = pages.iter().collect();
    sorted.sort_by(|x, y| {
        x.status()
            .cmp(&y.status())
            .then_with(|| {
                let distance = |p: &PageResult| p.distance().unwrap_or(0.0);
                distance(y).total_cmp(&distance(x))
            })
            .then_with(|| {
                let differing =
                    |p: &PageResult| p.comparison.as_ref().map_or(0, |c| c.differing_pixels);
                differing(y).cmp(&differing(x))
            })
            .then_with(|| x.n.cmp(&y.n))
    });
    sorted
}

/// Run the bench as `options` say.
pub fn run(options: &RunOptions) -> Result<Run, String> {
    let started = UtcTime::now();
    let engines = engine::load_engines(&options.engines_file)?;
    let spec = |name: &str| {
        engines.get(name).cloned().ok_or_else(|| {
            let known: Vec<&str> = engines.keys().map(String::as_str).collect();
            format!(
                "moteur inconnu : {name} ; {} décrit : {}",
                options.engines_file.display(),
                known.join(", ")
            )
        })
    };
    let (spec_a, spec_b) = (spec(&options.a)?, spec(&options.b)?);
    let metric = metric::by_name(&options.metric).ok_or_else(|| {
        format!(
            "métrique inconnue : {} ; au choix : {}",
            options.metric,
            metric::NAMES.join(", ")
        )
    })?;
    if options.repeat == 0 {
        return Err("--repeat : au moins une fois".to_string());
    }
    let set = PageSet::load(&options.pages_file)?;
    let pages_sha256 = std::fs::read(&options.pages_file)
        .map(|bytes| system::sha256_hex(&bytes))
        .map_err(|e| format!("{} : {e}", options.pages_file.display()))?;
    let places = Places {
        root: options.root.clone(),
        bin_dir: options.bin_dir.clone(),
    };
    if options.build {
        spec_a.build(&options.a, &places)?;
        if options.b != options.a {
            spec_b.build(&options.b, &places)?;
        }
    }
    for side in ["a", "b", "diff"] {
        let dir = options.out.join(side);
        std::fs::create_dir_all(&dir).map_err(|e| format!("{} : {e}", dir.display()))?;
    }

    let measured = options
        .limit
        .unwrap_or(set.pages.len())
        .min(set.pages.len());
    let subset = PageSet {
        pages: set.pages.iter().take(measured).cloned().collect(),
        ..set.clone()
    };
    let files = pageset::check_files(&subset, &options.root);
    let mut pages: Vec<PageResult> = subset
        .pages
        .iter()
        .enumerate()
        .map(|(position, entry)| PageResult {
            n: position + 1,
            entry: entry.clone(),
            file: files
                .get(&entry.file)
                .cloned()
                .unwrap_or(FileState::Unchanged),
            a: EngineResult::default(),
            b: EngineResult::default(),
            comparison: None,
            image_a: format!("a/{}", image_name(position + 1)),
            image_b: format!("b/{}", image_name(position + 1)),
            image_diff: None,
        })
        .collect();
    let mut sides = [
        EngineSide::new(&options.a, &spec_a),
        EngineSide::new(&options.b, &spec_b),
    ];
    if options.verbose {
        eprintln!(
            "Banc de fidélité : {} (A) contre {} (B), {} pages de {}, {} pixels de large, {} répétition(s), métrique {}",
            options.a,
            options.b,
            measured,
            system::relative(&options.root, &options.pages_file),
            set.width,
            options.repeat,
            metric.name()
        );
    }

    let rendering = Instant::now();
    let documents = subset.documents();
    for (k, document) in documents.iter().enumerate() {
        let state = files.get(&document.file);
        if state != Some(&FileState::Unchanged) {
            if options.verbose {
                eprintln!(
                    "[{:>4}/{}] {} : non mesuré ({})",
                    k + 1,
                    documents.len(),
                    document.file,
                    match state {
                        Some(FileState::Changed { .. }) => "fichier modifié depuis la sélection",
                        _ => "fichier absent",
                    }
                );
            }
            continue;
        }
        // The engine that goes first alternates: neither always gets the
        // colder caches of the system.
        let order = if k % 2 == 0 { [0, 1] } else { [1, 0] };
        let mut walls = [0.0; 2];
        for side in order {
            let (spec, dir) = if side == 0 {
                (&spec_a, "a")
            } else {
                (&spec_b, "b")
            };
            let request = Request {
                protocol: PROTOCOL,
                document: under(&options.root, &document.file),
                password: document.password.clone(),
                width: set.width,
                repeat: options.repeat,
                pages: document
                    .positions
                    .iter()
                    .map(|&position| PageRequest {
                        index: subset.pages[position].index,
                        output: options.out.join(dir).join(image_name(position + 1)),
                    })
                    .collect(),
            };
            let document_run =
                engine::run_document(spec.command(&places), &request, options.timeout);
            sides[side].absorb(&document_run);
            walls[side] = document_run.wall_ms;
            let stderr = (!document_run.stderr.trim().is_empty())
                .then(|| engine::shorten(document_run.stderr.trim(), 2000));
            for (&position, outcome) in document.positions.iter().zip(&document_run.pages) {
                let result = if side == 0 {
                    &mut pages[position].a
                } else {
                    &mut pages[position].b
                };
                result.open_ms = document_run.open_ms;
                result.stderr = matches!(outcome, PageOutcome::Failed { .. })
                    .then(|| stderr.clone())
                    .flatten();
                result.outcome = Some(outcome.clone());
            }
        }
        if options.verbose {
            let failed = |side: usize| {
                document
                    .positions
                    .iter()
                    .filter(|&&p| {
                        let result = if side == 0 { &pages[p].a } else { &pages[p].b };
                        !result.rendered()
                    })
                    .count()
            };
            let mut line = format!(
                "[{:>4}/{}] {}, {} page(s) : A {} s, B {} s",
                k + 1,
                documents.len(),
                document.file,
                document.positions.len(),
                number(walls[0] / 1000.0, 2),
                number(walls[1] / 1000.0, 2)
            );
            for (side, label) in [(0, "A"), (1, "B")] {
                if failed(side) > 0 {
                    line.push_str(&format!(" ; {label} : {} en échec", failed(side)));
                }
            }
            eprintln!("{line}");
        }
    }
    let render_s = rendering.elapsed().as_secs_f64();

    if options.verbose {
        eprintln!("Comparaison des images sur {} fil(s)…", options.jobs.max(1));
    }
    let comparing = Instant::now();
    compare_all(&mut pages, &options.out, metric.as_ref(), options.jobs);
    let compare_s = comparing.elapsed().as_secs_f64();

    let [a, b] = sides;
    Ok(Run {
        started: started.display(),
        machine: system::machine(),
        revision: system::git_revision(&options.root),
        rustc: system::rustc_version(&options.root),
        command_line: options.command_line.clone(),
        pages_file: system::relative(&options.root, &options.pages_file),
        pages_sha256,
        set_pages: set.pages.len(),
        width: set.width,
        repeat: options.repeat,
        timeout_s: options.timeout.as_secs(),
        metric: metric.name().to_string(),
        metric_description: metric.description().to_string(),
        a,
        b,
        pages,
        render_s,
        compare_s,
    })
}

/// `file`, a path with forward slashes relative to `root`, under `root`.
fn under(root: &Path, file: &str) -> PathBuf {
    file.split('/')
        .fold(root.to_path_buf(), |path, part| path.join(part))
}

/// Name of the image of the page at position `n` of the set.
fn image_name(n: usize) -> String {
    format!("{n:04}.png")
}

/// Compare the images of every page both engines drew, on `jobs` threads.
fn compare_all(pages: &mut [PageResult], out: &Path, metric: &dyn Metric, jobs: usize) {
    let next = AtomicUsize::new(0);
    let slots: Vec<Mutex<&mut PageResult>> = pages.iter_mut().map(Mutex::new).collect();
    std::thread::scope(|scope| {
        for _ in 0..jobs.max(1) {
            scope.spawn(|| {
                while let Some(slot) = slots.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let mut page = slot.lock().unwrap_or_else(PoisonError::into_inner);
                    compare_page(&mut page, out, metric);
                }
            });
        }
    });
}

fn compare_page(page: &mut PageResult, out: &Path, metric: &dyn Metric) {
    if page.status() != Status::Compared {
        return;
    }
    let (a, b) = (
        compare::load_png(&out.join(&page.image_a)),
        compare::load_png(&out.join(&page.image_b)),
    );
    match (a, b) {
        (Ok(a), Ok(b)) => {
            let (comparison, picture) = compare::compare(&a, &b, metric);
            let diff = format!("diff/{}", image_name(page.n));
            if save_png(&picture, &out.join(&diff)).is_ok() {
                page.image_diff = Some(diff);
            }
            page.comparison = Some(comparison);
        }
        (a, b) => {
            for (result, image) in [(&mut page.a, a), (&mut page.b, b)] {
                if let Err(error) = image {
                    result.outcome = Some(PageOutcome::Failed {
                        error: format!("le moteur dit avoir écrit l'image, mais : {error}"),
                    });
                }
            }
        }
    }
}

/// Write `image` as a PNG, quickly: the settings of the page service.
pub fn save_png(image: &RgbImage, path: &Path) -> Result<(), String> {
    let mut png = Vec::new();
    PngEncoder::new_with_quality(&mut png, CompressionType::Fast, FilterType::Up)
        .write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            ExtendedColorType::Rgb8,
        )
        .map_err(|e| format!("{} : {e}", path.display()))?;
    std::fs::write(path, png).map_err(|e| format!("{} : {e}", path.display()))
}

/// Why `run` fails the non-regression guard at `threshold`, one reason per
/// line; empty when it passes. It fails on a page B does not draw where A
/// does, on a distance above the threshold, on a page not measured, and on a
/// partial set: a guard that measures less than the set says nothing of the
/// rest.
pub fn guard(run: &Run, threshold: f64) -> Vec<String> {
    let mut reasons = Vec::new();
    if run.pages.len() < run.set_pages {
        reasons.push(format!(
            "jeu partiel : {} pages mesurées sur {}",
            run.pages.len(),
            run.set_pages
        ));
    }
    for page in &run.pages {
        let name = format!(
            "page {} ({}, page {})",
            page.n,
            page.entry.file,
            page.entry.index + 1
        );
        match page.status() {
            Status::NotMeasured => reasons.push(format!("{name} : non mesurée")),
            Status::BFailed => reasons.push(format!(
                "{name} : {} échoue là où {} réussit : {}",
                run.b.name,
                run.a.name,
                page.b.error().unwrap_or_default()
            )),
            Status::Compared => {
                if let Some(distance) = page.distance().filter(|d| *d > threshold) {
                    reasons.push(format!(
                        "{name} : distance {} au-dessus du seuil {}",
                        number(distance, 6),
                        number(threshold, 6)
                    ));
                }
            }
            Status::AFailed | Status::BothFailed => {}
        }
    }
    reasons
}
