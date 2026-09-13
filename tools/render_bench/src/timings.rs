//! Reference times of an engine on the page set, `tools/render_bench/timings/`:
//! per page, the median time to draw and to encode, with their extremes, and
//! what they were measured with. They hold on the machine that measured them,
//! with the same build: the `[run]` table says which.

use serde::{Deserialize, Serialize};

use crate::engine::PageOutcome;
use crate::run::{EngineResult, Run, Status};
use crate::stats;

/// A reference timings file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimingsFile {
    /// What was measured, where, with what.
    pub run: TimingsRun,
    /// Every page of the set, in its order.
    #[serde(rename = "page", default)]
    pub pages: Vec<PageTimings>,
}

/// The circumstances of the measure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimingsRun {
    /// When the run started.
    pub date: String,
    /// The engine.
    pub engine: String,
    /// Its version, as it gave it.
    pub version: String,
    /// What else it said: the library loaded.
    pub detail: String,
    /// Processor, threads and system.
    pub machine: String,
    /// Compiler of the engine.
    pub rustc: String,
    /// Commit of the checkout.
    pub revision: String,
    /// The page set file.
    pub pages_file: String,
    /// SHA-256 of the page set file.
    pub pages_sha256: String,
    /// Width of the images, in pixels.
    pub width: u32,
    /// Repetitions of each page per process.
    pub repeat: u32,
    /// Processes of this engine per document whose samples are merged: 2
    /// when the run had it on both sides.
    pub processes: u32,
}

/// The times of one page, in milliseconds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PageTimings {
    /// Position in the set, from 1.
    pub n: usize,
    /// The file.
    pub file: String,
    /// Page number, 0-based.
    pub index: usize,
    /// Width of the image, in pixels.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    /// Height of the image, in pixels.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    /// Median time to open its document, over the processes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_ms: Option<f64>,
    /// Median time to draw the page, over every sample.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_ms: Option<f64>,
    /// Shortest time to draw it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_min_ms: Option<f64>,
    /// Longest time to draw it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_max_ms: Option<f64>,
    /// Median time to encode the page as a PNG.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encode_ms: Option<f64>,
    /// Shortest time to encode it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encode_min_ms: Option<f64>,
    /// Longest time to encode it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encode_max_ms: Option<f64>,
    /// Samples behind these figures.
    pub samples: usize,
    /// Why the page has no times, when it has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// The reference times of engine `name` in `run`, from every process of that
/// engine: A and B alike when both sides are it.
pub fn from_run(run: &Run, name: &str) -> Result<TimingsFile, String> {
    let sides = [(run.a.name == name, &run.a), (run.b.name == name, &run.b)];
    let Some(side) = sides.iter().find(|(used, _)| *used).map(|(_, side)| *side) else {
        return Err(format!("le moteur {name} n'a pas tourné pendant ce banc"));
    };
    let info = side
        .info
        .clone()
        .unwrap_or_else(|| crate::engine::EngineInfo {
            name: name.to_string(),
            version: "inconnue".to_string(),
            detail: String::new(),
        });
    let used: Vec<bool> = sides.iter().map(|(used, _)| *used).collect();
    let pages = run
        .pages
        .iter()
        .map(|page| {
            let results: Vec<&EngineResult> = [&page.a, &page.b]
                .into_iter()
                .zip(&used)
                .filter(|(_, used)| **used)
                .map(|(result, _)| result)
                .collect();
            let (mut render, mut encode, mut open) = (Vec::new(), Vec::new(), Vec::new());
            let mut size = None;
            let mut error = None;
            for result in &results {
                open.extend(result.open_ms);
                match &result.outcome {
                    Some(PageOutcome::Rendered {
                        width,
                        height,
                        render_ms,
                        encode_ms,
                        ..
                    }) => {
                        size.get_or_insert((*width, *height));
                        render.extend(render_ms);
                        encode.extend(encode_ms);
                    }
                    Some(PageOutcome::Failed { error: e }) => {
                        error.get_or_insert_with(|| e.clone());
                    }
                    None => {}
                }
            }
            if page.status() == Status::NotMeasured {
                error = Some("non mesurée : fichier absent ou modifié".to_string());
            }
            PageTimings {
                n: page.n,
                file: page.entry.file.clone(),
                index: page.entry.index,
                width: size.map(|s| s.0),
                height: size.map(|s| s.1),
                open_ms: stats::median(&open).map(round),
                render_ms: stats::median(&render).map(round),
                render_min_ms: stats::min(&render).map(round),
                render_max_ms: stats::max(&render).map(round),
                encode_ms: stats::median(&encode).map(round),
                encode_min_ms: stats::min(&encode).map(round),
                encode_max_ms: stats::max(&encode).map(round),
                samples: render.len(),
                error: if render.is_empty() { error } else { None },
            }
        })
        .collect();
    Ok(TimingsFile {
        run: TimingsRun {
            date: run.started.clone(),
            engine: name.to_string(),
            version: info.version,
            detail: info.detail,
            machine: run.machine.clone(),
            rustc: run.rustc.clone(),
            revision: run.revision.clone(),
            pages_file: run.pages_file.clone(),
            pages_sha256: run.pages_sha256.clone(),
            width: run.width,
            repeat: run.repeat,
            processes: u32::try_from(used.iter().filter(|u| **u).count()).unwrap_or(1),
        },
        pages,
    })
}

/// A thousandth of a millisecond is below what the measure can tell.
fn round(ms: f64) -> f64 {
    (ms * 1000.0).round() / 1000.0
}

impl TimingsFile {
    /// The file as TOML, with the comment that says how to read it.
    pub fn to_toml(&self) -> Result<String, String> {
        let body = toml::to_string(self).map_err(|e| format!("temps de référence : {e}"))?;
        Ok(format!(
            "# Temps de référence du moteur {} sur le jeu de pages du banc de fidélité\n\
             # du rendu (docs/banc-rendu.md, « Temps de référence »). Écrit par\n\
             # `fyp-render-bench run --record-timings` ; ne pas modifier à la main.\n\
             #\n\
             # Ils ne valent que sur la machine et avec le build de la table [run] :\n\
             # comparés à des temps pris ailleurs, ils ne mesurent que la différence\n\
             # des machines. En millisecondes ; pour chaque page, la médiane des\n\
             # échantillons de tous les processus de ce moteur, et leurs extrêmes.\n\n{body}",
            self.run.engine
        ))
    }

    /// Read a reference timings file.
    pub fn parse(text: &str) -> Result<TimingsFile, String> {
        toml::from_str(text).map_err(|e| format!("temps de référence illisibles : {e}"))
    }
}
