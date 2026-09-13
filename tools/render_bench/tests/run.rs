//! Runs of the bench from end to end, with the fake engine: the report and
//! its ranking, the failures an engine may have, the guard, the reference
//! times.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::time::{Duration, Instant};

use fyp_render_bench::pageset::{PageEntry, PageSet, FORMAT};
use fyp_render_bench::report::{self, Summary};
use fyp_render_bench::run::{self, worst_first, Run, RunOptions, Status};
use fyp_render_bench::system;
use fyp_render_bench::timings::{self, TimingsFile};

/// Engines `fake` (well-behaved) and `other` (the fake engine with `args`).
fn engines(args: &[&str]) -> String {
    let program = env!("CARGO_BIN_EXE_fyp-render-engine-fake").replace('\\', "/");
    let args: String = args.iter().map(|arg| format!(", '{arg}'")).collect();
    format!(
        "[fake]\ndescription = \"moteur factice\"\ncommand = ['{program}']\n\n[other]\ndescription = \"moteur factice qui se comporte mal\"\ncommand = ['{program}'{args}]\n"
    )
}

/// A scratch directory with a page set of `pages` and the engines of
/// `engines`, and the options of a run of `fake` against `other` there.
fn bench(name: &str, engines: &str, pages: &[(&str, usize)]) -> RunOptions {
    let root = system::repository_root();
    let dir = std::env::temp_dir().join(format!("fyp-render-bench-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let set = PageSet {
        format: FORMAT,
        width: 64,
        pages: pages
            .iter()
            .map(|(file, index)| PageEntry {
                file: file.to_string(),
                sha256: system::sha256_hex(&std::fs::read(root.join(file)).unwrap()),
                index: *index,
                password: String::new(),
                why: vec!["test".to_string()],
            })
            .collect(),
    };
    std::fs::write(dir.join("pages.toml"), set.to_toml("Jeu de test.").unwrap()).unwrap();
    std::fs::write(dir.join("engines.toml"), engines).unwrap();
    RunOptions {
        root,
        bin_dir: dir.clone(),
        engines_file: dir.join("engines.toml"),
        pages_file: dir.join("pages.toml"),
        a: "fake".to_string(),
        b: "other".to_string(),
        metric: "ssim".to_string(),
        repeat: 2,
        limit: None,
        out: dir.join("run"),
        timeout: Duration::from_secs(30),
        build: false,
        jobs: 2,
        verbose: false,
        command_line: format!("test {name}"),
    }
}

const MINIMAL: &str = "tests/fixtures/minimal.pdf";
const OBJSTM: &str = "tests/fixtures/objstm.pdf";

fn b_error(run: &Run, n: usize) -> String {
    run.pages[n - 1].b.error().unwrap_or_default().to_string()
}

#[test]
fn the_same_engine_twice_is_at_distance_zero() {
    let mut options = bench(
        "same",
        &engines(&[]),
        &[(MINIMAL, 0), (MINIMAL, 1), (OBJSTM, 0)],
    );
    options.b = "fake".to_string();
    let run = run::run(&options).unwrap();
    assert_eq!(run.pages.len(), 3);
    for page in &run.pages {
        assert_eq!(page.status(), Status::Compared);
        let comparison = page.comparison.as_ref().unwrap();
        assert!(comparison.identical);
        assert_eq!(comparison.distance, 0.0);
        assert!(options.out.join(page.image_diff.as_ref().unwrap()).exists());
    }
    let (good, verdict) = Summary::of(&run).verdict();
    assert_eq!(good, Some(true), "{verdict}");
    assert!(verdict.contains("déterministe"), "{verdict}");

    let index = report::write(&run, &options.out).unwrap();
    let html = std::fs::read_to_string(index).unwrap();
    assert!(html.contains("Banc de fidélité du rendu"));
    assert!(html.contains("a/0001.png"));
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(options.out.join("results.json")).unwrap())
            .unwrap();
    assert_eq!(json["pages"].as_array().unwrap().len(), 3);

    // Both sides are the same engine: its reference times merge both
    // processes, two repetitions each.
    let file = timings::from_run(&run, "fake").unwrap();
    assert_eq!(file.run.processes, 2);
    assert!(file
        .pages
        .iter()
        .all(|page| page.samples == 4 && page.render_ms.is_some()));
    assert_eq!(TimingsFile::parse(&file.to_toml().unwrap()).unwrap(), file);
    assert!(timings::from_run(&run, "other").is_err());
}

#[test]
fn a_moved_block_is_measured_and_ranked_first() {
    let options = bench(
        "moved",
        &engines(&["--shift-page", "1"]),
        &[(MINIMAL, 0), (MINIMAL, 1), (MINIMAL, 2)],
    );
    let run = run::run(&options).unwrap();
    let distances: Vec<f64> = run.pages.iter().map(|p| p.distance().unwrap()).collect();
    assert_eq!(distances[0], 0.0);
    assert!(distances[1] > 0.0);
    assert_eq!(distances[2], 0.0);
    assert_eq!(worst_first(&run.pages)[0].n, 2);
    let reasons = run::guard(&run, 0.0);
    assert_eq!(reasons.len(), 1, "{reasons:?}");
    assert!(reasons[0].starts_with("page 2 "), "{reasons:?}");
    assert!(run::guard(&run, 1.0).is_empty());
    assert_eq!(Summary::of(&run).verdict().0, None);
}

#[test]
fn a_crash_costs_the_rest_of_its_document_only() {
    let options = bench(
        "crash",
        &engines(&["--crash-after", "1"]),
        &[(MINIMAL, 0), (MINIMAL, 1), (MINIMAL, 2), (OBJSTM, 0)],
    );
    let run = run::run(&options).unwrap();
    let statuses: Vec<Status> = run.pages.iter().map(|p| p.status()).collect();
    assert_eq!(
        statuses,
        [
            Status::Compared,
            Status::BFailed,
            Status::BFailed,
            Status::Compared
        ]
    );
    assert!(
        b_error(&run, 2).contains("s'effondre"),
        "{}",
        b_error(&run, 2)
    );
    assert!(run.pages[1]
        .b
        .stderr
        .as_deref()
        .unwrap_or("")
        .contains("s'effondre"));
    let worst: Vec<usize> = worst_first(&run.pages).iter().map(|p| p.n).collect();
    assert_eq!(worst[..2], [2, 3]);
    assert!(run::guard(&run, 1.0)
        .iter()
        .any(|reason| reason.contains("échoue")));
    let html = report::html(&run);
    assert!(html.contains("s&#39;effondre") || html.contains("s'effondre"));
}

#[test]
fn a_silent_engine_is_stopped() {
    let mut options = bench("silent", &engines(&["--hang-after", "0"]), &[(MINIMAL, 0)]);
    options.timeout = Duration::from_secs(2);
    let started = Instant::now();
    let run = run::run(&options).unwrap();
    assert!(started.elapsed() < Duration::from_secs(20));
    assert_eq!(run.pages[0].status(), Status::BFailed);
    assert!(
        b_error(&run, 1).contains("délai dépassé"),
        "{}",
        b_error(&run, 1)
    );
}

#[test]
fn answers_outside_the_protocol_fail_the_pages() {
    for (args, words) in [
        (&["--garbage"][..], "illisible"),
        (&["--protocol", "7"][..], "protocole 7"),
        (&["--fail-open"][..], "ouverture refusée"),
        (&["--fail-page", "0"][..], "page refusée"),
    ] {
        let options = bench("protocol", &engines(args), &[(MINIMAL, 0)]);
        let run = run::run(&options).unwrap();
        assert_eq!(run.pages[0].status(), Status::BFailed, "{args:?}");
        assert!(
            b_error(&run, 1).contains(words),
            "{args:?}: {}",
            b_error(&run, 1)
        );
    }
}

#[test]
fn a_page_whose_file_changed_is_not_measured() {
    let options = bench("changed", &engines(&[]), &[(MINIMAL, 0), (OBJSTM, 0)]);
    let text = std::fs::read_to_string(&options.pages_file).unwrap();
    let digest =
        system::sha256_hex(&std::fs::read(system::repository_root().join(OBJSTM)).unwrap());
    std::fs::write(&options.pages_file, text.replace(&digest, &"0".repeat(64))).unwrap();
    let run = run::run(&options).unwrap();
    assert_eq!(run.pages[1].status(), Status::NotMeasured);
    assert_eq!(Summary::of(&run).not_measured, 1);
    assert!(run::guard(&run, 1.0)
        .iter()
        .any(|reason| reason.contains("non mesurée")));
}

#[test]
fn a_partial_run_is_said_to_be_partial() {
    let mut options = bench(
        "partial",
        &engines(&[]),
        &[(MINIMAL, 0), (MINIMAL, 1), (OBJSTM, 0)],
    );
    options.limit = Some(1);
    let run = run::run(&options).unwrap();
    assert_eq!((run.pages.len(), run.set_pages), (1, 3));
    assert!(run::guard(&run, 1.0)
        .iter()
        .any(|reason| reason.contains("jeu partiel")));
}

#[test]
fn unknown_engines_and_metrics_are_refused() {
    let mut options = bench("unknown", &engines(&[]), &[(MINIMAL, 0)]);
    options.b = "nope".to_string();
    assert!(run::run(&options)
        .unwrap_err()
        .contains("moteur inconnu : nope"));
    options.b = "fake".to_string();
    options.metric = "psnr".to_string();
    assert!(run::run(&options)
        .unwrap_err()
        .contains("métrique inconnue"));
    let _ = PathBuf::new();
}
