//! What a run found, for a reader and for a program: `index.html`, the pages
//! from the worst to the best under a summary in figures, with both images of
//! each page and a picture of their differences; `results.json`, the whole
//! run.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::engine::PageOutcome;
use crate::run::{worst_first, EngineResult, EngineSide, PageResult, Run, Status};
use crate::stats;

/// `value` with `decimals` decimals and a decimal comma, as the French
/// documentation writes numbers.
pub fn number(value: f64, decimals: usize) -> String {
    format!("{value:.decimals$}").replace('.', ",")
}

/// A distance as the report writes it: `0` when it is exactly zero.
pub fn distance_text(distance: f64) -> String {
    if distance == 0.0 {
        "0".to_string()
    } else if distance < 0.01 {
        number(distance, 6)
    } else {
        number(distance, 4)
    }
}

/// Times of one engine, over the pages both engines drew.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EngineTimes {
    /// Time to open each document, once per document, in milliseconds.
    pub open_ms: Vec<f64>,
    /// Median time to draw each page, in milliseconds.
    pub render_ms: Vec<f64>,
    /// Median time to encode each page, in milliseconds.
    pub encode_ms: Vec<f64>,
}

/// The figures at the top of a report.
#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    /// Pages the run measured, or tried to.
    pub pages: usize,
    /// Pages of the set.
    pub set_pages: usize,
    /// Pages whose file is missing or changed.
    pub not_measured: usize,
    /// Pages both engines drew.
    pub compared: usize,
    /// Of those, pages whose images are identical.
    pub identical: usize,
    /// Pages A did not draw where B did.
    pub a_failed: usize,
    /// Pages B did not draw where A did.
    pub b_failed: usize,
    /// Pages neither drew.
    pub both_failed: usize,
    /// Compared pages whose sizes differ by more than a pixel.
    pub size_mismatches: usize,
    /// Distance of every compared page.
    pub distances: Vec<f64>,
    /// Pages whose repetitions by A gave different pixels.
    pub unstable_a: usize,
    /// Pages whose repetitions by B gave different pixels.
    pub unstable_b: usize,
    /// Times of A.
    pub a: EngineTimes,
    /// Times of B.
    pub b: EngineTimes,
    /// Whether A and B are the same engine: a measure of determinism and of
    /// the noise of the times.
    pub same_engine: bool,
    /// When they are, `|B / A − 1|` of the median time to draw, per page.
    pub render_noise: Vec<f64>,
}

impl Summary {
    /// The summary of `run`.
    pub fn of(run: &Run) -> Summary {
        let count = |status: Status| run.pages.iter().filter(|p| p.status() == status).count();
        let compared: Vec<&PageResult> = run
            .pages
            .iter()
            .filter(|p| p.status() == Status::Compared)
            .collect();
        let unstable = |result: fn(&PageResult) -> &EngineResult| {
            run.pages
                .iter()
                .filter(|p| {
                    matches!(
                        result(p).outcome,
                        Some(PageOutcome::Rendered {
                            identical_repeats: false,
                            ..
                        })
                    )
                })
                .count()
        };
        let times = |result: fn(&PageResult) -> &EngineResult| {
            let mut times = EngineTimes::default();
            let mut documents = BTreeSet::new();
            for page in &compared {
                let side = result(page);
                if documents.insert((page.entry.file.as_str(), page.entry.password.as_str())) {
                    times.open_ms.extend(side.open_ms);
                }
                times.render_ms.extend(side.render_ms());
                times.encode_ms.extend(side.encode_ms());
            }
            times
        };
        let same_engine = run.a.name == run.b.name;
        let render_noise = if same_engine {
            compared
                .iter()
                .filter_map(|p| match (p.a.render_ms(), p.b.render_ms()) {
                    (Some(a), Some(b)) if a > 0.0 => Some((b / a - 1.0).abs()),
                    _ => None,
                })
                .collect()
        } else {
            Vec::new()
        };
        Summary {
            pages: run.pages.len(),
            set_pages: run.set_pages,
            not_measured: count(Status::NotMeasured),
            compared: compared.len(),
            identical: compared
                .iter()
                .filter(|p| p.comparison.as_ref().is_some_and(|c| c.identical))
                .count(),
            a_failed: count(Status::AFailed),
            b_failed: count(Status::BFailed),
            both_failed: count(Status::BothFailed),
            size_mismatches: compared
                .iter()
                .filter(|p| {
                    p.comparison
                        .as_ref()
                        .is_some_and(|c| c.distance == 1.0 && c.note.is_some())
                })
                .count(),
            distances: compared.iter().filter_map(|p| p.distance()).collect(),
            unstable_a: unstable(|p| &p.a),
            unstable_b: unstable(|p| &p.b),
            a: times(|p| &p.a),
            b: times(|p| &p.b),
            same_engine,
            render_noise,
        }
    }

    /// What the run says in one sentence, and whether it is good news.
    pub fn verdict(&self) -> (Option<bool>, String) {
        if self.compared == 0 {
            return (
                Some(false),
                "Aucune page comparée : aucune page dessinée par les deux moteurs.".to_string(),
            );
        }
        let unstable = self.unstable_a + self.unstable_b;
        let mut text;
        let good;
        if self.same_engine {
            let differing = self.compared - self.identical;
            if differing == 0 && unstable == 0 {
                good = Some(true);
                text = format!(
                    "Rendu déterministe : les {} pages comparées sont identiques au pixel près d'un processus à l'autre, et d'une répétition à l'autre dans chaque processus. Écart nul.",
                    self.compared
                );
            } else {
                good = Some(false);
                text = format!(
                    "Rendu NON déterministe : {differing} page(s) sur {} diffèrent d'un processus à l'autre, {unstable} d'une répétition à l'autre dans un même processus. Distance maximale : {}.",
                    self.compared,
                    distance_text(stats::max(&self.distances).unwrap_or(0.0))
                );
            }
        } else {
            good = None;
            text = format!(
                "{} page(s) identiques sur {} comparées ; distance médiane {}, 90e centile {}, maximale {}.",
                self.identical,
                self.compared,
                distance_text(stats::median(&self.distances).unwrap_or(0.0)),
                distance_text(stats::percentile(&self.distances, 90.0).unwrap_or(0.0)),
                distance_text(stats::max(&self.distances).unwrap_or(0.0))
            );
        }
        let failures = self.a_failed + self.b_failed + self.both_failed;
        if failures > 0 {
            let _ = write!(text, " {failures} page(s) en échec d'au moins un moteur.");
        }
        if self.not_measured > 0 {
            let _ = write!(
                text,
                " {} page(s) non mesurées : fichier absent ou modifié.",
                self.not_measured
            );
        }
        if self.pages < self.set_pages {
            let _ = write!(
                text,
                " Jeu partiel : {} pages sur {}.",
                self.pages, self.set_pages
            );
        }
        (good, text)
    }
}

/// Write `index.html` and `results.json` of `run` into `dir`; the path of the
/// page.
pub fn write(run: &Run, dir: &Path) -> Result<PathBuf, String> {
    let json = serde_json::to_string_pretty(run).map_err(|e| format!("results.json : {e}"))?;
    let results = dir.join("results.json");
    std::fs::write(&results, json).map_err(|e| format!("{} : {e}", results.display()))?;
    let index = dir.join("index.html");
    std::fs::write(&index, html(run)).map_err(|e| format!("{} : {e}", index.display()))?;
    Ok(index)
}

/// `latest.html` in `runs`, which opens the report in `run_dir`, a
/// sub-directory of `runs`.
pub fn write_latest(runs: &Path, run_dir: &Path) -> Result<PathBuf, String> {
    let target = format!("{}/index.html", crate::system::relative(runs, run_dir));
    let page = format!(
        "<!doctype html>\n<meta charset=\"utf-8\">\n<meta http-equiv=\"refresh\" content=\"0; url={0}\">\n<title>Dernier banc de fidélité</title>\n<a href=\"{0}\">{0}</a>\n",
        escape(&target)
    );
    let latest = runs.join("latest.html");
    std::fs::write(&latest, page).map_err(|e| format!("{} : {e}", latest.display()))?;
    Ok(latest)
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

const STYLE: &str = r#"
:root { --bg: #fafaf8; --fg: #1d1d1f; --muted: #60646c; --line: #dcdcd7; --card: #ffffff; --good: #1e7b34; --bad: #b3261e; --warn: #8a5a00; --link: #2f5bb7; }
@media (prefers-color-scheme: dark) { :root { --bg: #161617; --fg: #ececec; --muted: #a2a2a8; --line: #3a3a3d; --card: #1f1f21; --good: #6fcf86; --bad: #ff8a80; --warn: #f0b35a; --link: #8ab4f8; } }
body { margin: 0; background: var(--bg); color: var(--fg); font: 14px/1.45 system-ui, -apple-system, "Segoe UI", Roboto, sans-serif; }
main { max-width: 1480px; margin: 0 auto; padding: 24px; }
a { color: var(--link); }
h1 { font-size: 22px; margin: 0 0 4px; }
h2 { font-size: 17px; margin: 32px 0 10px; }
.sub, .muted { color: var(--muted); }
.verdict { border-left: 5px solid var(--muted); background: var(--card); padding: 12px 16px; margin: 18px 0; font-size: 15px; }
.verdict.good { border-color: var(--good); }
.verdict.bad { border-color: var(--bad); }
.cards { display: grid; grid-template-columns: repeat(auto-fit, minmax(340px, 1fr)); gap: 18px; align-items: start; }
.scroll { overflow-x: auto; }
table { border-collapse: collapse; width: 100%; background: var(--card); }
th, td { border-bottom: 1px solid var(--line); padding: 6px 9px; text-align: left; vertical-align: top; }
th { font-weight: 600; }
.num { text-align: right; font-variant-numeric: tabular-nums; white-space: nowrap; }
.pages td.images { white-space: nowrap; }
.pages figure { display: inline-block; margin: 0 8px 0 0; vertical-align: top; }
.pages figcaption { font-size: 11px; color: var(--muted); }
.pages img { width: 230px; height: auto; border: 1px solid var(--line); background: #fff; display: block; }
.tag { display: inline-block; font-size: 11px; color: var(--muted); border: 1px solid var(--line); border-radius: 3px; padding: 0 4px; margin: 3px 3px 0 0; }
.error { color: var(--bad); max-width: 460px; white-space: normal; }
.warn { color: var(--warn); }
details pre { white-space: pre-wrap; font-size: 12px; max-height: 260px; overflow: auto; }
dl { display: grid; grid-template-columns: max-content 1fr; gap: 4px 16px; margin: 0; }
dt { color: var(--muted); }
dd { margin: 0; overflow-wrap: anywhere; }
"#;

/// The report as one HTML page.
pub fn html(run: &Run) -> String {
    let summary = Summary::of(run);
    let mut out = String::new();
    let title = format!(
        "Banc de fidélité du rendu : {} contre {}",
        run.a.name, run.b.name
    );
    let _ = writeln!(
        out,
        "<!doctype html>\n<html lang=\"fr\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>{}</title>\n<style>{STYLE}</style>\n</head>\n<body>\n<main>",
        escape(&title)
    );
    let _ = writeln!(
        out,
        "<h1>Banc de fidélité du rendu</h1>\n<p class=\"sub\">A : <b>{}</b> — B : <b>{}</b> — {} — {} pages de <code>{}</code>, {} pixels de large, métrique <b>{}</b>, {} répétition(s)</p>",
        escape(&run.a.name),
        escape(&run.b.name),
        escape(&run.started),
        summary.pages,
        escape(&run.pages_file),
        run.width,
        escape(&run.metric),
        run.repeat
    );
    let (good, verdict) = summary.verdict();
    let class = match good {
        Some(true) => "verdict good",
        Some(false) => "verdict bad",
        None => "verdict",
    };
    let _ = writeln!(out, "<p class=\"{class}\">{}</p>", escape(&verdict));

    let _ = writeln!(out, "<div class=\"cards\">");
    pages_card(&mut out, &summary);
    distance_card(&mut out, &summary, run);
    let _ = writeln!(out, "</div>");
    times_card(&mut out, &summary, run);
    pages_table(&mut out, run);
    run_card(&mut out, run);
    let _ = writeln!(out, "</main>\n</body>\n</html>");
    out
}

fn pages_card(out: &mut String, summary: &Summary) {
    let rows = [
        ("Pages du jeu", summary.set_pages.to_string()),
        ("Pages mesurées", summary.pages.to_string()),
        (
            "Non mesurées (fichier absent ou modifié)",
            summary.not_measured.to_string(),
        ),
        (
            "Comparées : dessinées par les deux moteurs",
            summary.compared.to_string(),
        ),
        ("Identiques au pixel près", summary.identical.to_string()),
        ("Échec de B seul", summary.b_failed.to_string()),
        ("Échec de A seul", summary.a_failed.to_string()),
        ("Échec des deux", summary.both_failed.to_string()),
        (
            "Tailles différentes de plus d'un pixel",
            summary.size_mismatches.to_string(),
        ),
        (
            "Répétitions différentes dans un même processus (A / B)",
            format!("{} / {}", summary.unstable_a, summary.unstable_b),
        ),
    ];
    let _ = writeln!(out, "<section><h2>Pages</h2><table>");
    for (label, value) in rows {
        let _ = writeln!(
            out,
            "<tr><td>{label}</td><td class=\"num\">{value}</td></tr>"
        );
    }
    let _ = writeln!(out, "</table></section>");
}

fn distance_card(out: &mut String, summary: &Summary, run: &Run) {
    let d = &summary.distances;
    let _ = writeln!(
        out,
        "<section><h2>Distance ({})</h2><p class=\"muted\">{}</p><table>",
        escape(&run.metric),
        escape(&run.metric_description)
    );
    let stat = |value: Option<f64>| value.map_or("—".to_string(), distance_text);
    for (label, value) in [
        ("Moyenne", stat(stats::mean(d))),
        ("Médiane", stat(stats::median(d))),
        ("90e centile", stat(stats::percentile(d, 90.0))),
        ("Maximum", stat(stats::max(d))),
    ] {
        let _ = writeln!(
            out,
            "<tr><td>{label}</td><td class=\"num\">{value}</td></tr>"
        );
    }
    let buckets: [Bucket; 6] = [
        ("= 0", |x| x == 0.0),
        ("≤ 0,001", |x| x > 0.0 && x <= 0.001),
        ("≤ 0,01", |x| x > 0.001 && x <= 0.01),
        ("≤ 0,05", |x| x > 0.01 && x <= 0.05),
        ("≤ 0,2", |x| x > 0.05 && x <= 0.2),
        ("> 0,2", |x| x > 0.2),
    ];
    for (label, inside) in buckets {
        let _ = writeln!(
            out,
            "<tr><td>Pages à distance {label}</td><td class=\"num\">{}</td></tr>",
            d.iter().filter(|x| inside(**x)).count()
        );
    }
    let _ = writeln!(out, "</table></section>");
}

/// A range of distances in the summary: its label, and whether a distance
/// falls in it.
type Bucket = (&'static str, fn(f64) -> bool);

/// A row of the table of times: its label, the statistic, and the times it
/// reads.
type TimeRow = (
    &'static str,
    fn(&[f64]) -> Option<f64>,
    fn(&EngineTimes) -> &[f64],
);

fn times_card(out: &mut String, summary: &Summary, run: &Run) {
    let (a, b) = (&summary.a, &summary.b);
    let _ = writeln!(
        out,
        "<section><h2>Temps</h2><p class=\"muted\">En millisecondes, sur les {} pages dessinées par les deux moteurs. Rendu : de la page demandée à ses pixels en mémoire ; encodage : ces pixels en PNG, comme le service de pages de l'application. Par page, la médiane de ses {} répétition(s). L'ouverture d'un document compte une fois par document.</p><div class=\"scroll\"><table>",
        summary.compared, run.repeat
    );
    let _ = writeln!(
        out,
        "<tr><th></th><th class=\"num\">A : {}</th><th class=\"num\">B : {}</th><th class=\"num\">B / A</th></tr>",
        escape(&run.a.name),
        escape(&run.b.name)
    );
    let ms = |value: Option<f64>| value.map_or("—".to_string(), |v| number(v, 1));
    let ratio = |x: Option<f64>, y: Option<f64>| match (x, y) {
        (Some(x), Some(y)) if x > 0.0 => number(y / x, 2),
        _ => "—".to_string(),
    };
    let sum = |values: &[f64]| (!values.is_empty()).then(|| values.iter().sum::<f64>());
    let rows: [TimeRow; 9] = [
        (
            "Ouverture des documents, somme",
            |v| (!v.is_empty()).then(|| v.iter().sum()),
            |t| &t.open_ms,
        ),
        ("Ouverture, médiane par document", stats::median, |t| {
            &t.open_ms
        }),
        (
            "Rendu, somme des pages",
            |v| (!v.is_empty()).then(|| v.iter().sum()),
            |t| &t.render_ms,
        ),
        ("Rendu, médiane par page", stats::median, |t| &t.render_ms),
        (
            "Rendu, 90e centile",
            |v| stats::percentile(v, 90.0),
            |t| &t.render_ms,
        ),
        ("Rendu, maximum", stats::max, |t| &t.render_ms),
        (
            "Encodage PNG, somme des pages",
            |v| (!v.is_empty()).then(|| v.iter().sum()),
            |t| &t.encode_ms,
        ),
        ("Encodage PNG, médiane par page", stats::median, |t| {
            &t.encode_ms
        }),
        (
            "Encodage PNG, 90e centile",
            |v| stats::percentile(v, 90.0),
            |t| &t.encode_ms,
        ),
    ];
    for (label, statistic, values) in rows {
        let (x, y) = (statistic(values(a)), statistic(values(b)));
        let _ = writeln!(
            out,
            "<tr><td>{label}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td></tr>",
            ms(x),
            ms(y),
            ratio(x, y)
        );
    }
    let share = |t: &EngineTimes| match (sum(&t.render_ms), sum(&t.encode_ms)) {
        (Some(r), Some(e)) if r + e > 0.0 => format!("{} %", number(100.0 * e / (r + e), 1)),
        _ => "—".to_string(),
    };
    let _ = writeln!(
        out,
        "<tr><td>Part de l'encodage dans rendu + encodage</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td></td></tr>",
        share(a),
        share(b)
    );
    let process = |side: &EngineSide| number(side.process_ms / 1000.0, 1);
    let _ = writeln!(
        out,
        "<tr><td>Processus des moteurs, de leur lancement à leur fin, en secondes, tous documents</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td></tr>",
        process(&run.a),
        process(&run.b),
        ratio(Some(run.a.process_ms), Some(run.b.process_ms))
    );
    let _ = writeln!(out, "</table></div>");
    if summary.same_engine {
        let noise = &summary.render_noise;
        let percent = |value: Option<f64>| {
            value.map_or("—".to_string(), |v| format!("{} %", number(100.0 * v, 1)))
        };
        let _ = writeln!(
            out,
            "<p>Même moteur des deux côtés : l'écart entre ses deux mesures d'une page est le bruit de la mesure. Rendu, |B / A − 1| par page : médiane {}, 90e centile {}, maximum {}.</p>",
            percent(stats::median(noise)),
            percent(stats::percentile(noise, 90.0)),
            percent(stats::max(noise))
        );
    }
    let _ = writeln!(out, "</section>");
}

fn pages_table(out: &mut String, run: &Run) {
    let _ = writeln!(
        out,
        "<section><h2>Pages, de la pire à la meilleure</h2><p class=\"muted\">Échecs de B, puis de A, puis des deux ; puis les pages comparées par distance décroissante ; les pages non mesurées à la fin. L'image de différence montre A en gris, et en rouge chaque pixel qui diffère, d'autant plus vif que l'écart est grand ; en magenta, la surface d'une seule des deux images.</p><div class=\"scroll\"><table class=\"pages\">"
    );
    let _ = writeln!(
        out,
        "<tr><th class=\"num\">Rang</th><th>Page</th><th class=\"num\">Distance</th><th class=\"num\">Pixels différents</th><th class=\"num\">Taille</th><th class=\"num\">Rendu (ms)</th><th class=\"num\">Encodage (ms)</th><th>Images : A, B, différence</th></tr>"
    );
    for (rank, page) in worst_first(&run.pages).into_iter().enumerate() {
        page_row(out, rank + 1, page, run);
    }
    let _ = writeln!(out, "</table></div></section>");
}

fn page_row(out: &mut String, rank: usize, page: &PageResult, run: &Run) {
    let mut tags = String::new();
    for why in &page.entry.why {
        let _ = write!(tags, "<span class=\"tag\">{}</span>", escape(why));
    }
    let password = if page.entry.password.is_empty() {
        String::new()
    } else {
        format!(", mot de passe « {} »", escape(&page.entry.password))
    };
    let (distance, differing) = match (&page.comparison, page.status()) {
        (Some(c), _) => {
            let share = if c.compared_pixels > 0 {
                100.0 * c.differing_pixels as f64 / c.compared_pixels as f64
            } else {
                0.0
            };
            let note = c
                .note
                .as_ref()
                .map(|n| format!("<br><span class=\"warn\">{}</span>", escape(n)))
                .unwrap_or_default();
            (
                format!("{}{note}", distance_text(c.distance)),
                format!(
                    "{}<br><span class=\"muted\">{} %</span>",
                    c.differing_pixels,
                    number(share, 3)
                ),
            )
        }
        (None, Status::BFailed) => (
            format!(
                "<span class=\"error\">échec de {}</span>",
                escape(&run.b.name)
            ),
            String::new(),
        ),
        (None, Status::AFailed) => (
            format!(
                "<span class=\"error\">échec de {}</span>",
                escape(&run.a.name)
            ),
            String::new(),
        ),
        (None, Status::BothFailed) => (
            "<span class=\"error\">échec des deux</span>".to_string(),
            String::new(),
        ),
        (None, Status::NotMeasured) => (
            "<span class=\"warn\">non mesurée</span>".to_string(),
            String::new(),
        ),
        (None, Status::Compared) => ("—".to_string(), String::new()),
    };
    let size = |result: &EngineResult| match &result.outcome {
        Some(PageOutcome::Rendered { width, height, .. }) => format!("{width} × {height}"),
        _ => "—".to_string(),
    };
    let ms = |value: Option<f64>| value.map_or("—".to_string(), |v| number(v, 1));
    let _ = write!(
        out,
        "<tr><td class=\"num\">{rank}</td><td><b>{}.</b> {}<br>page {}{password}<br>{tags}</td><td class=\"num\">{distance}</td><td class=\"num\">{differing}</td><td class=\"num\">A {}<br>B {}</td><td class=\"num\">A {}<br>B {}</td><td class=\"num\">A {}<br>B {}</td><td class=\"images\">",
        page.n,
        escape(&page.entry.file),
        page.entry.index + 1,
        size(&page.a),
        size(&page.b),
        ms(page.a.render_ms()),
        ms(page.b.render_ms()),
        ms(page.a.encode_ms()),
        ms(page.b.encode_ms()),
    );
    match &page.file {
        crate::pageset::FileState::Missing { error } => {
            let _ = write!(
                out,
                "<div class=\"warn\">Fichier absent : {}</div>",
                escape(error)
            );
        }
        crate::pageset::FileState::Changed { sha256 } => {
            let _ = write!(
                out,
                "<div class=\"warn\">Fichier modifié depuis la sélection : SHA-256 {}, le jeu attend {}</div>",
                escape(sha256),
                escape(&page.entry.sha256)
            );
        }
        crate::pageset::FileState::Unchanged => {
            for (label, result, image, name) in [
                ("A", &page.a, &page.image_a, &run.a.name),
                ("B", &page.b, &page.image_b, &run.b.name),
            ] {
                if result.rendered() {
                    figure(out, &format!("{label} : {name}"), image);
                } else if let Some(error) = result.error() {
                    let _ = write!(
                        out,
                        "<div class=\"error\">{label} ({}) : {}</div>",
                        escape(name),
                        escape(error)
                    );
                    if let Some(stderr) = &result.stderr {
                        let _ = write!(
                            out,
                            "<details><summary>Sortie d'erreur de {label}</summary><pre>{}</pre></details>",
                            escape(stderr)
                        );
                    }
                }
            }
            if let Some(diff) = &page.image_diff {
                figure(out, "différence", diff);
            }
        }
    }
    let _ = writeln!(out, "</td></tr>");
}

fn figure(out: &mut String, caption: &str, image: &str) {
    let _ = write!(
        out,
        "<figure><a href=\"{0}\"><img loading=\"lazy\" src=\"{0}\" alt=\"{1}\"></a><figcaption>{1}</figcaption></figure>",
        escape(image),
        escape(caption)
    );
}

fn run_card(out: &mut String, run: &Run) {
    let _ = writeln!(out, "<section><h2>Exécution</h2><dl>");
    let mut row = |label: &str, value: String| {
        let _ = writeln!(out, "<dt>{label}</dt><dd>{value}</dd>");
    };
    row("Date", escape(&run.started));
    row(
        "Commande",
        format!("<code>{}</code>", escape(&run.command_line)),
    );
    row("Machine", escape(&run.machine));
    row("Révision du dépôt", escape(&run.revision));
    row("Compilateur", escape(&run.rustc));
    row(
        "Jeu de pages",
        format!(
            "<code>{}</code>, SHA-256 {}, {} pages, {} pixels de large",
            escape(&run.pages_file),
            escape(run.pages_sha256.get(..16).unwrap_or(&run.pages_sha256)),
            run.set_pages,
            run.width
        ),
    );
    row("Répétitions par page", run.repeat.to_string());
    row(
        "Délai sans réponse d'un moteur",
        format!("{} s", run.timeout_s),
    );
    row(
        "Durées",
        format!(
            "rendu {} s, comparaison {} s",
            number(run.render_s, 1),
            number(run.compare_s, 1)
        ),
    );
    for (label, side) in [("Moteur A", &run.a), ("Moteur B", &run.b)] {
        let mut text = format!(
            "<b>{}</b> : {}",
            escape(&side.name),
            escape(&side.description)
        );
        if let Some(info) = &side.info {
            let _ = write!(
                text,
                "<br>version : {}<br>{}",
                escape(&info.version),
                escape(&info.detail)
            );
        }
        for other in &side.other_infos {
            let _ = write!(
                text,
                "<br><span class=\"warn\">Autre présentation sur un autre document : {}, {}</span>",
                escape(&other.version),
                escape(&other.detail)
            );
        }
        let _ = write!(text, "<br>{} document(s)", side.documents);
        row(label, text);
    }
    let _ = writeln!(out, "</dl><p class=\"muted\">Tout le détail : <a href=\"results.json\">results.json</a>.</p></section>");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_written_the_french_way() {
        assert_eq!(number(12.345, 1), "12,3");
        assert_eq!(number(0.5, 0), "0");
        assert_eq!(distance_text(0.0), "0");
        assert_eq!(distance_text(0.000_012_3), "0,000012");
        assert_eq!(distance_text(0.25), "0,2500");
        assert_eq!(escape("<a & \"b\">"), "&lt;a &amp; &quot;b&quot;&gt;");
    }
}
