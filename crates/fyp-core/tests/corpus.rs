//! Survey of `tests/corpus/`: every PDF found there is opened, written back
//! in both cross-reference styles and read again. Nothing is asserted per
//! file; the outcome of each is collected into a report that says what to
//! fix first. The directory is empty in CI (the suites are fetched locally
//! by `tools/fetch_corpus.py`), so an empty corpus is a pass.
//!
//! Report: `target/corpus-report.md` (override with `FYP_CORPUS_REPORT`).
//! Per-file time limit: 60 s (override with `FYP_CORPUS_TIMEOUT_SECS`).
//! `FYP_CORPUS_STRICT=1` makes the test fail on any problem, for the day
//! the whole corpus round-trips.

#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use fyp_core::document::Document;
use fyp_core::writer::{Writer, XrefStyle};

/// What the corpus files say about one file.
#[derive(Debug, Clone)]
enum Outcome {
    /// Opened, both round trips passed. Carries the repair reason when the
    /// declared table had to be rebuilt.
    Passed {
        repaired: Option<String>,
        /// `startxref` pointed elsewhere and the section was found nearby.
        relocated: bool,
    },
    /// A git-lfs pointer whose content was never fetched: not a PDF.
    LfsPointer,
    OpenFailed(String),
    /// Opened (possibly repaired), then a round trip stage failed.
    TripFailed {
        repaired: Option<String>,
        stage: Stage,
        style: XrefStyle,
        message: String,
    },
    Panicked(String),
    TimedOut,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Stage {
    Write,
    Reopen,
    ReopenRepaired,
    Mismatch,
    Unstable,
}

impl Stage {
    fn label(self) -> &'static str {
        match self {
            Stage::Write => "écriture refusée",
            Stage::Reopen => "relecture impossible",
            Stage::ReopenRepaired => "relecture avec reconstruction",
            Stage::Mismatch => "objets différents après round-trip",
            Stage::Unstable => "seconde écriture différente",
        }
    }
}

struct Record {
    path: String,
    bytes: u64,
    elapsed: Duration,
    outcome: Outcome,
}

/// Open, write in both styles, reopen, compare, write again.
fn examine(bytes: &[u8]) -> Outcome {
    if bytes.starts_with(b"version https://git-lfs.github.com/spec/") {
        return Outcome::LfsPointer;
    }
    let doc = match Document::open(bytes) {
        Ok(doc) => doc,
        Err(e) => return Outcome::OpenFailed(e.to_string()),
    };
    let repaired = doc.reconstructed().map(ToString::to_string);
    for style in [XrefStyle::Table, XrefStyle::Stream] {
        let failed = |stage, message: String| Outcome::TripFailed {
            repaired: repaired.clone(),
            stage,
            style,
            message,
        };
        let writer = Writer::new(doc.version()).xref_style(style);
        let out = match writer.write(&doc) {
            Ok(out) => out,
            Err(e) => return failed(Stage::Write, e.to_string()),
        };
        let again = match Document::open(&out) {
            Ok(again) => again,
            Err(e) => return failed(Stage::Reopen, e.to_string()),
        };
        if let Some(reason) = again.reconstructed() {
            return failed(Stage::ReopenRepaired, reason.to_string());
        }
        if let Err(difference) = common::compare(&doc, &again) {
            return failed(Stage::Mismatch, difference);
        }
        match writer.write(&again) {
            Ok(second) if second == out => {}
            Ok(_) => return failed(Stage::Unstable, "bytes differ".into()),
            Err(e) => return failed(Stage::Unstable, e.to_string()),
        }
    }
    Outcome::Passed {
        repaired,
        relocated: doc.relocated_startxref().is_some(),
    }
}

/// Run `examine` on another thread so that a panic or a hang in the core
/// becomes a line of the report instead of the end of the survey.
fn examine_with_limits(bytes: Vec<u8>, timeout: Duration) -> Outcome {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let outcome = match panic::catch_unwind(AssertUnwindSafe(|| examine(&bytes))) {
            Ok(outcome) => outcome,
            Err(payload) => Outcome::Panicked(
                payload
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| payload.downcast_ref::<&str>().map(ToString::to_string))
                    .unwrap_or_else(|| "non-string panic payload".into()),
            ),
        };
        let _ = tx.send(outcome);
    });
    match rx.recv_timeout(timeout) {
        Ok(outcome) => outcome,
        Err(mpsc::RecvTimeoutError::Timeout) => Outcome::TimedOut,
        Err(mpsc::RecvTimeoutError::Disconnected) => Outcome::Panicked("thread died".into()),
    }
}

/// Group key for a message: digits and quoted text vary from file to file,
/// the rest names the kind of problem.
fn normalize(message: &str) -> String {
    let mut out = String::new();
    let mut in_quote = false;
    let mut last_digit = false;
    for c in message.chars() {
        if c == '"' {
            in_quote = !in_quote;
            out.push_str(if in_quote { "\"" } else { "…\"" });
            continue;
        }
        if in_quote {
            continue;
        }
        if c.is_ascii_digit() {
            if !last_digit {
                out.push('N');
            }
            last_digit = true;
        } else {
            last_digit = false;
            out.push(c);
        }
    }
    out
}

struct Group {
    count: usize,
    sample_message: String,
    files: Vec<String>,
}

fn grouped<'a>(items: impl Iterator<Item = (&'a str, &'a str)>) -> Vec<(String, Group)> {
    let mut groups: BTreeMap<String, Group> = BTreeMap::new();
    for (path, message) in items {
        let group = groups.entry(normalize(message)).or_insert_with(|| Group {
            count: 0,
            sample_message: message.to_string(),
            files: Vec::new(),
        });
        group.count += 1;
        if group.files.len() < 8 {
            group.files.push(path.to_string());
        }
    }
    let mut v: Vec<(String, Group)> = groups.into_iter().collect();
    v.sort_by(|a, b| b.1.count.cmp(&a.1.count).then_with(|| a.0.cmp(&b.0)));
    v
}

fn write_groups(report: &mut String, groups: &[(String, Group)]) {
    for (key, group) in groups {
        let _ = writeln!(report, "\n### {} — {}", group.count, key);
        if group.sample_message != *key {
            let _ = writeln!(report, "\nExemple : `{}`\n", group.sample_message);
        }
        for file in &group.files {
            let _ = writeln!(report, "- `{file}`");
        }
        if group.count > group.files.len() {
            let _ = writeln!(
                report,
                "- … et {} autre(s)",
                group.count - group.files.len()
            );
        }
    }
}

fn style_name(style: XrefStyle) -> &'static str {
    match style {
        XrefStyle::Table => "table",
        XrefStyle::Stream => "flux",
    }
}

fn report(records: &[Record], corpus: &Path, total: Duration) -> String {
    let count =
        |pred: &dyn Fn(&Outcome) -> bool| records.iter().filter(|r| pred(&r.outcome)).count();
    let sound = count(&|o| matches!(o, Outcome::Passed { repaired: None, .. }));
    let repaired_ok = count(&|o| {
        matches!(
            o,
            Outcome::Passed {
                repaired: Some(_),
                ..
            }
        )
    });
    let relocated = count(&|o| {
        matches!(
            o,
            Outcome::Passed {
                relocated: true,
                ..
            }
        )
    });
    let repaired_any = records
        .iter()
        .filter(|r| match &r.outcome {
            Outcome::Passed { repaired, .. } | Outcome::TripFailed { repaired, .. } => {
                repaired.is_some()
            }
            _ => false,
        })
        .count();
    let open_failed = count(&|o| matches!(o, Outcome::OpenFailed(_)));
    let trip_failed = count(&|o| matches!(o, Outcome::TripFailed { .. }));
    let panicked = count(&|o| matches!(o, Outcome::Panicked(_)));
    let timed_out = count(&|o| matches!(o, Outcome::TimedOut));
    let lfs = count(&|o| matches!(o, Outcome::LfsPointer));
    let bytes: u64 = records.iter().map(|r| r.bytes).sum();

    let mut out = String::new();
    let _ = writeln!(out, "# Rapport corpus\n");
    let _ = writeln!(
        out,
        "Dossier `{}`, {} fichiers, {:.1} Mio, {:.1} s.\n",
        corpus.display(),
        records.len(),
        bytes as f64 / (1024.0 * 1024.0),
        total.as_secs_f64()
    );
    let _ = writeln!(out, "| Résultat | Fichiers |\n|---|---:|");
    let _ = writeln!(
        out,
        "| Ouverts sans reconstruction, round-trip complet | {sound} |"
    );
    let _ = writeln!(
        out,
        "| Réparés (xref reconstruite), round-trip complet | {repaired_ok} |"
    );
    let _ = writeln!(out, "| Refusés à l'ouverture | {open_failed} |");
    let _ = writeln!(out, "| Ouverts mais round-trip en échec | {trip_failed} |");
    let _ = writeln!(out, "| Panique du noyau | {panicked} |");
    let _ = writeln!(out, "| Délai dépassé | {timed_out} |");
    let _ = writeln!(out, "| Pointeurs git-lfs non récupérés | {lfs} |");
    let _ = writeln!(out, "\nFichiers dont la xref a dû être reconstruite, tous résultats confondus : {repaired_any}.");
    let _ = writeln!(out, "\nFichiers ouverts sans reconstruction mais dont `startxref` pointait à côté de la table : {relocated}.");

    let mut by_lot: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    for r in records {
        let lot = r.path.split('/').next().unwrap_or("");
        let entry = by_lot.entry(lot).or_default();
        entry.0 += 1;
        if matches!(r.outcome, Outcome::Passed { .. }) {
            entry.1 += 1;
        }
    }
    if by_lot.len() > 1 {
        let _ = writeln!(
            out,
            "\n| Lot | Fichiers | Round-trip complet |\n|---|---:|---:|"
        );
        for (lot, (n, ok)) in &by_lot {
            let _ = writeln!(out, "| `{lot}` | {n} | {ok} |");
        }
    }

    let _ = writeln!(out, "\n## Priorité 1 — paniques et délais dépassés\n");
    let mut urgent = 0;
    for r in records {
        match &r.outcome {
            Outcome::Panicked(message) => {
                urgent += 1;
                let _ = writeln!(out, "- PANIQUE `{}` : {message}", r.path);
            }
            Outcome::TimedOut => {
                urgent += 1;
                let _ = writeln!(out, "- DÉLAI `{}` ({} octets)", r.path, r.bytes);
            }
            _ => {}
        }
    }
    if urgent == 0 {
        let _ = writeln!(out, "Aucun.");
    }

    let _ = writeln!(out, "\n## Priorité 2 — refus à l'ouverture, par cause\n");
    let groups = grouped(records.iter().filter_map(|r| match &r.outcome {
        Outcome::OpenFailed(m) => Some((r.path.as_str(), m.as_str())),
        _ => None,
    }));
    if groups.is_empty() {
        let _ = writeln!(out, "Aucun.");
    }
    write_groups(&mut out, &groups);

    let _ = writeln!(
        out,
        "\n## Priorité 3 — round-trip en échec, par étape et cause\n"
    );
    let mut any = false;
    for stage in [
        Stage::Write,
        Stage::Reopen,
        Stage::ReopenRepaired,
        Stage::Mismatch,
        Stage::Unstable,
    ] {
        let messages: Vec<(String, &str)> = records
            .iter()
            .filter_map(|r| match &r.outcome {
                Outcome::TripFailed {
                    stage: s,
                    style,
                    message,
                    ..
                } if *s == stage => Some((
                    format!("{} ({})", r.path, style_name(*style)),
                    message.as_str(),
                )),
                _ => None,
            })
            .collect();
        if messages.is_empty() {
            continue;
        }
        any = true;
        let _ = writeln!(
            out,
            "\n### Étape : {} — {} fichier(s)",
            stage.label(),
            messages.len()
        );
        let groups = grouped(messages.iter().map(|(p, m)| (p.as_str(), *m)));
        write_groups(&mut out, &groups);
    }
    if !any {
        let _ = writeln!(out, "Aucun.");
    }

    let _ = writeln!(out, "\n## Priorité 4 — tables reconstruites, par cause\n");
    let _ = writeln!(
        out,
        "Le fichier s'ouvre, mais sa table déclarée a été rejetée. Une cause fréquente sur des fichiers que d'autres lecteurs ouvrent sans réparation signale une tolérance manquante dans `xref` ou `document::verify`.\n"
    );
    let groups = grouped(records.iter().filter_map(|r| match &r.outcome {
        Outcome::Passed {
            repaired: Some(m), ..
        }
        | Outcome::TripFailed {
            repaired: Some(m), ..
        } => Some((r.path.as_str(), m.as_str())),
        _ => None,
    }));
    if groups.is_empty() {
        let _ = writeln!(out, "Aucune.");
    }
    write_groups(&mut out, &groups);

    let _ = writeln!(out, "\n## Priorité 5 — `startxref` corrigé\n");
    let _ = writeln!(
        out,
        "Le fichier s'ouvre sur sa table déclarée, trouvée près de l'offset annoncé (décalage de quelques octets, en-tête précédé de déchets).\n"
    );
    let mut any_relocated = false;
    for r in records {
        if matches!(
            r.outcome,
            Outcome::Passed {
                relocated: true,
                ..
            }
        ) {
            any_relocated = true;
            let _ = writeln!(out, "- `{}`", r.path);
        }
    }
    if !any_relocated {
        let _ = writeln!(out, "Aucun.");
    }

    let _ = writeln!(out, "\n## Les plus lents\n");
    let mut slowest: Vec<&Record> = records.iter().collect();
    slowest.sort_by_key(|r| std::cmp::Reverse(r.elapsed));
    let _ = writeln!(out, "| Fichier | Octets | Secondes |\n|---|---:|---:|");
    for r in slowest.iter().take(10) {
        let _ = writeln!(
            out,
            "| `{}` | {} | {:.2} |",
            r.path,
            r.bytes,
            r.elapsed.as_secs_f64()
        );
    }

    let _ = writeln!(out, "\n## Tous les fichiers\n");
    let _ = writeln!(
        out,
        "| Fichier | Octets | Résultat | Détail |\n|---|---:|---|---|"
    );
    for r in records {
        let (result, detail) = match &r.outcome {
            Outcome::Passed {
                repaired: None,
                relocated,
            } => (
                "ok",
                if *relocated {
                    "startxref corrigé".to_string()
                } else {
                    String::new()
                },
            ),
            Outcome::Passed {
                repaired: Some(m), ..
            } => ("réparé", m.clone()),
            Outcome::LfsPointer => ("pointeur lfs", String::new()),
            Outcome::OpenFailed(m) => ("ouverture", m.clone()),
            Outcome::TripFailed {
                stage,
                style,
                message,
                ..
            } => (
                "round-trip",
                format!("{} ({}) : {message}", stage.label(), style_name(*style)),
            ),
            Outcome::Panicked(m) => ("PANIQUE", m.clone()),
            Outcome::TimedOut => ("DÉLAI", String::new()),
        };
        let _ = writeln!(
            out,
            "| `{}` | {} | {result} | {} |",
            r.path,
            r.bytes,
            detail.replace('|', "\\|").replace('\n', " ")
        );
    }
    out
}

fn report_path(workspace: &Path) -> PathBuf {
    std::env::var_os("FYP_CORPUS_REPORT")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace.join("target/corpus-report.md"))
}

#[test]
fn corpus_survey() {
    let corpus = common::tests_dir().join("corpus");
    let files = common::pdf_files_recursive(&corpus);
    if files.is_empty() {
        eprintln!(
            "no PDF under {}: run tools/fetch_corpus.py to get the public suites",
            corpus.display()
        );
        return;
    }
    let timeout = std::env::var("FYP_CORPUS_TIMEOUT_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .map_or(Duration::from_secs(60), Duration::from_secs);

    let started = Instant::now();
    let mut records = Vec::with_capacity(files.len());
    for path in &files {
        let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let rel = path
            .strip_prefix(&corpus)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        let t0 = Instant::now();
        let outcome = examine_with_limits(bytes.clone(), timeout);
        records.push(Record {
            path: rel,
            bytes: bytes.len() as u64,
            elapsed: t0.elapsed(),
            outcome,
        });
    }
    let total = started.elapsed();

    let text = report(&records, &corpus, total);
    let workspace = common::tests_dir().join("..");
    let out = report_path(&workspace);
    if let Some(parent) = out.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(&out, &text).unwrap_or_else(|e| panic!("{}: {e}", out.display()));

    let summary_end = text.find("\n## Priorité 1").unwrap_or(text.len());
    eprintln!("{}", &text[..summary_end]);
    eprintln!("rapport complet : {}", out.display());

    if std::env::var_os("FYP_CORPUS_STRICT").is_some() {
        let problems = records
            .iter()
            .filter(|r| !matches!(r.outcome, Outcome::Passed { .. } | Outcome::LfsPointer))
            .count();
        assert_eq!(
            problems,
            0,
            "{problems} corpus file(s) failed, see {}",
            out.display()
        );
    }
}
