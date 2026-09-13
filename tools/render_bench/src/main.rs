//! `fyp-render-bench`, the rendering fidelity bench (`docs/banc-rendu.md`).
//!
//! ```text
//! cargo run --release -p fyp-render-bench                       # PDFium contre PDFium
//! cargo run --release -p fyp-render-bench -- run --b <moteur>   # un autre moteur contre PDFium
//! cargo run --release -p fyp-render-bench -- select             # choisir de nouveau le jeu de pages
//! cargo run --release -p fyp-render-bench -- engines            # les moteurs connus
//! ```

#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use clap::{Args, Parser, Subcommand};
use fyp_render_bench::report::{self, Summary};
use fyp_render_bench::run::{self, RunOptions};
use fyp_render_bench::select::{self, ChooseOptions, SelectOptions};
use fyp_render_bench::{engine, pageset, stats, system, timings};

/// Exit code of a run that trips the non-regression guard.
const GUARD_TRIPPED: u8 = 3;

#[derive(Parser)]
#[command(
    name = "fyp-render-bench",
    about = "Banc de fidélité du rendu : deux moteurs, un jeu de pages, l'écart et les temps de chaque page (docs/banc-rendu.md)",
    args_conflicts_with_subcommands = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
    #[command(flatten)]
    run: RunArgs,
}

#[derive(Subcommand)]
enum Command {
    /// Rendre le jeu de pages avec deux moteurs, comparer les images, écrire le rapport (commande par défaut)
    Run(RunArgs),
    /// Choisir un jeu de pages dans les fixtures et le corpus public
    Select(SelectArgs),
    /// Lister les moteurs du fichier des moteurs
    Engines {
        /// Fichier des moteurs [défaut : tools/render_bench/engines.toml]
        #[arg(long)]
        engines: Option<PathBuf>,
    },
}

#[derive(Args, Clone)]
struct RunArgs {
    /// Moteur A, la référence
    #[arg(long, default_value = "pdfium")]
    a: String,
    /// Moteur B, mesuré contre A
    #[arg(long, default_value = "pdfium")]
    b: String,
    /// Jeu de pages [défaut : tools/render_bench/pages.toml]
    #[arg(long)]
    pages: Option<PathBuf>,
    /// Fichier des moteurs [défaut : tools/render_bench/engines.toml]
    #[arg(long)]
    engines: Option<PathBuf>,
    /// Métrique : ssim ou pixels
    #[arg(long, default_value = "ssim")]
    metric: String,
    /// Rendus et encodages de chaque page, par moteur
    #[arg(long, default_value_t = 3)]
    repeat: u32,
    /// Seulement les N premières pages du jeu : un aperçu, pas une mesure comparable
    #[arg(long, value_name = "N")]
    limit: Option<usize>,
    /// Dossier des rapports [défaut : target/render-bench]
    #[arg(long)]
    out: Option<PathBuf>,
    /// Secondes sans réponse avant d'arrêter un moteur
    #[arg(long, default_value_t = 120)]
    timeout: u64,
    /// Ne pas construire les moteurs avant de les lancer
    #[arg(long)]
    no_build: bool,
    /// Fils de comparaison des images [défaut : tous]
    #[arg(long)]
    jobs: Option<usize>,
    /// Écrire les temps de référence du moteur A dans ce fichier
    #[arg(long, value_name = "FICHIER")]
    record_timings: Option<PathBuf>,
    /// Garde de non-régression : sortir en erreur (code 3) si une page dépasse cette distance, si B échoue là où A réussit, ou si une page n'est pas mesurée
    #[arg(long, value_name = "DISTANCE")]
    fail_above: Option<f64>,
    /// Accepter un build debug, dont les temps ne valent rien
    #[arg(long)]
    allow_debug: bool,
}

#[derive(Args, Clone)]
struct SelectArgs {
    /// Moteur de référence : une page du corpus n'entre que s'il la rend
    #[arg(long, default_value = "pdfium")]
    reference: String,
    /// Fichier des moteurs [défaut : tools/render_bench/engines.toml]
    #[arg(long)]
    engines: Option<PathBuf>,
    /// Jeu de pages à écrire [défaut : tools/render_bench/pages.toml]
    #[arg(long)]
    out: Option<PathBuf>,
    /// Largeur des images, en pixels
    #[arg(long, default_value_t = 1400)]
    width: u32,
    /// Pages voulues par étiquette
    #[arg(long, default_value_t = 2)]
    per_feature: usize,
    /// Pages au plus par fichier
    #[arg(long, default_value_t = 2)]
    max_per_file: usize,
    /// Pages tirées au hasard, graine fixe, dans chaque lot du corpus
    #[arg(long, default_value_t = 8)]
    sample_per_lot: usize,
    /// Pages les plus lourdes que la référence rend dans le temps maximal, une par fichier : des temps de pages chargées
    #[arg(long, default_value_t = 16)]
    heavy: usize,
    /// Candidates essayées au plus pour une étiquette
    #[arg(long, default_value_t = 6)]
    attempts: usize,
    /// Pages examinées au plus par fichier
    #[arg(long, default_value_t = 8)]
    max_pages_scanned: usize,
    /// Temps de rendu maximal d'une page retenue, en millisecondes
    #[arg(long, default_value_t = 3000.0)]
    max_render_ms: f64,
    /// Secondes sans réponse avant d'arrêter le moteur
    #[arg(long, default_value_t = 120)]
    timeout: u64,
    /// Ne pas construire le moteur avant de le lancer
    #[arg(long)]
    no_build: bool,
    /// Fils d'examen des fichiers [défaut : tous]
    #[arg(long)]
    jobs: Option<usize>,
    /// Accepter un build debug
    #[arg(long)]
    allow_debug: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        None => run_bench(&cli.run),
        Some(Command::Run(args)) => run_bench(&args),
        Some(Command::Select(args)) => select_pages(&args),
        Some(Command::Engines { engines }) => list_engines(engines),
    };
    match result {
        Ok(code) => code,
        Err(error) => {
            eprintln!("fyp-render-bench : {error}");
            ExitCode::FAILURE
        }
    }
}

/// The checkout, the directory of this executable, and the target directory
/// that holds it.
struct Places {
    root: PathBuf,
    bin_dir: PathBuf,
    target: PathBuf,
}

fn places() -> Result<Places, String> {
    let root = system::repository_root();
    let exe =
        std::env::current_exe().map_err(|e| format!("exécutable du banc introuvable : {e}"))?;
    let bin_dir = exe
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "dossier de l'exécutable du banc introuvable".to_string())?;
    let target = bin_dir
        .parent()
        .map_or_else(|| root.join("target"), Path::to_path_buf);
    Ok(Places {
        root,
        bin_dir,
        target,
    })
}

fn release_only(allow_debug: bool) -> Result<(), String> {
    if cfg!(debug_assertions) && !allow_debug {
        return Err("le banc mesure des temps : compilez-le en release (cargo run --release -p fyp-render-bench), ou passez --allow-debug pour un essai dont les temps ne valent rien".to_string());
    }
    Ok(())
}

fn threads(jobs: Option<usize>) -> usize {
    jobs.unwrap_or_else(|| {
        std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get)
    })
}

fn run_bench(args: &RunArgs) -> Result<ExitCode, String> {
    release_only(args.allow_debug)?;
    let places = places()?;
    let runs = args
        .out
        .clone()
        .unwrap_or_else(|| places.target.join("render-bench"));
    let dir = runs.join(format!(
        "{}-{}-{}",
        system::UtcTime::now().stamp(),
        args.a,
        args.b
    ));
    let bench = places.root.join("tools").join("render_bench");
    let options = RunOptions {
        root: places.root.clone(),
        bin_dir: places.bin_dir.clone(),
        engines_file: args
            .engines
            .clone()
            .unwrap_or_else(|| bench.join("engines.toml")),
        pages_file: args
            .pages
            .clone()
            .unwrap_or_else(|| bench.join("pages.toml")),
        a: args.a.clone(),
        b: args.b.clone(),
        metric: args.metric.clone(),
        repeat: args.repeat,
        limit: args.limit,
        out: dir.clone(),
        timeout: Duration::from_secs(args.timeout),
        build: !args.no_build,
        jobs: threads(args.jobs),
        verbose: true,
        command_line: std::env::args().collect::<Vec<_>>().join(" "),
    };
    let run = run::run(&options)?;
    let index = report::write(&run, &dir)?;
    let _ = report::write_latest(&runs, &dir);
    let summary = Summary::of(&run);
    eprintln!("{}", summary.verdict().1);
    let seconds = |values: &[f64]| report::number(values.iter().sum::<f64>() / 1000.0, 1);
    eprintln!(
        "Temps des pages comparées : rendu A {} s, B {} s ; encodage PNG A {} s, B {} s ; rendu médian par page A {} ms, B {} ms",
        seconds(&summary.a.render_ms),
        seconds(&summary.b.render_ms),
        seconds(&summary.a.encode_ms),
        seconds(&summary.b.encode_ms),
        report::number(stats::median(&summary.a.render_ms).unwrap_or(0.0), 1),
        report::number(stats::median(&summary.b.render_ms).unwrap_or(0.0), 1)
    );
    eprintln!(
        "Durée : rendu {} s, comparaison {} s",
        report::number(run.render_s, 1),
        report::number(run.compare_s, 1)
    );
    eprintln!("Rapport : {}", index.display());
    if let Some(path) = &args.record_timings {
        let text = timings::from_run(&run, &args.a)?.to_toml()?;
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|e| format!("{} : {e}", parent.display()))?;
        }
        std::fs::write(path, text).map_err(|e| format!("{} : {e}", path.display()))?;
        eprintln!("Temps de référence de {} : {}", args.a, path.display());
    }
    if let Some(threshold) = args.fail_above {
        let reasons = run::guard(&run, threshold);
        if !reasons.is_empty() {
            eprintln!(
                "Garde de non-régression : échec, {} raison(s)",
                reasons.len()
            );
            for reason in reasons.iter().take(30) {
                eprintln!("  - {reason}");
            }
            return Ok(ExitCode::from(GUARD_TRIPPED));
        }
        eprintln!(
            "Garde de non-régression : aucune page au-delà de {}",
            report::number(threshold, 6)
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn select_pages(args: &SelectArgs) -> Result<ExitCode, String> {
    release_only(args.allow_debug)?;
    if !pageset::WIDTHS.contains(&args.width) {
        return Err(format!(
            "--width {} hors de {}..={}",
            args.width,
            pageset::WIDTHS.start(),
            pageset::WIDTHS.end()
        ));
    }
    let places = places()?;
    let bench = places.root.join("tools").join("render_bench");
    let out = args.out.clone().unwrap_or_else(|| bench.join("pages.toml"));
    let options = SelectOptions {
        root: places.root.clone(),
        bin_dir: places.bin_dir.clone(),
        engines_file: args
            .engines
            .clone()
            .unwrap_or_else(|| bench.join("engines.toml")),
        reference: args.reference.clone(),
        work: places.target.join("render-bench").join("select"),
        width: args.width,
        choose: ChooseOptions {
            per_feature: args.per_feature,
            max_per_file: args.max_per_file,
            sample_per_lot: args.sample_per_lot,
            heavy: args.heavy,
            attempts: args.attempts,
        },
        max_pages_scanned: args.max_pages_scanned,
        max_render_ms: args.max_render_ms,
        timeout: Duration::from_secs(args.timeout),
        build: !args.no_build,
        jobs: threads(args.jobs),
        verbose: true,
    };
    let (set, comment) = select::select(&options)?;
    std::fs::write(&out, set.to_toml(&comment)?).map_err(|e| format!("{} : {e}", out.display()))?;
    eprintln!("{} pages écrites dans {}", set.pages.len(), out.display());
    Ok(ExitCode::SUCCESS)
}

fn list_engines(engines: Option<PathBuf>) -> Result<ExitCode, String> {
    let file = engines.unwrap_or_else(|| {
        system::repository_root()
            .join("tools")
            .join("render_bench")
            .join("engines.toml")
    });
    for (name, spec) in engine::load_engines(&file)? {
        println!("{name:<12} {}", spec.description);
    }
    Ok(ExitCode::SUCCESS)
}
