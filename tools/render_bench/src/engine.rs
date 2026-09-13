//! The engines the bench knows (`engines.toml`) and how it runs one on a
//! document. An engine is a program started once per document: it receives a
//! [`Request`] and answers [`Reply`] lines ([`crate::protocol`]). Every
//! document gets its own process, so that a crash or a hang of an engine on
//! one file costs the pages of that file, never the rest of the run.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::protocol::{Reply, Request, PROTOCOL};

/// An engine, as `engines.toml` describes it under its name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EngineSpec {
    /// One line for the report: what draws the pages, through what.
    pub description: String,
    /// Command that builds the engine, run from the root of the repository;
    /// empty when there is nothing to build.
    #[serde(default)]
    pub build: Vec<String>,
    /// Command that runs the engine, arguments included.
    pub command: Vec<String>,
}

/// Read the engines of an `engines.toml`, by name.
pub fn load_engines(path: &Path) -> Result<BTreeMap<String, EngineSpec>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{} : {e}", path.display()))?;
    parse_engines(&text).map_err(|e| format!("{} : {e}", path.display()))
}

/// The engines described by `text`, by name.
pub fn parse_engines(text: &str) -> Result<BTreeMap<String, EngineSpec>, String> {
    let engines: BTreeMap<String, EngineSpec> =
        toml::from_str(text).map_err(|e| format!("moteurs illisibles : {e}"))?;
    for (name, spec) in &engines {
        let plain = name
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-');
        if name.is_empty() || !plain {
            return Err(format!(
                "moteur {name:?} : un nom fait de minuscules, de chiffres et de tirets"
            ));
        }
        if spec.command.is_empty() {
            return Err(format!("moteur {name} : commande vide"));
        }
    }
    Ok(engines)
}

/// What the placeholders of a command stand for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Places {
    /// `{root}`: the root of the repository, where commands run.
    pub root: PathBuf,
    /// `{bin_dir}`: the directory of the bench's executable.
    pub bin_dir: PathBuf,
}

impl Places {
    fn expand(&self, word: &str) -> String {
        word.replace("{bin_dir}", &self.bin_dir.to_string_lossy())
            .replace("{root}", &self.root.to_string_lossy())
    }
}

impl EngineSpec {
    /// Run the build command of the engine called `name`, if it has one, from
    /// the root; what it prints goes to the terminal.
    pub fn build(&self, name: &str, places: &Places) -> Result<(), String> {
        let Some((program, args)) = self.build.split_first() else {
            return Ok(());
        };
        let status = Command::new(places.expand(program))
            .args(args.iter().map(|arg| places.expand(arg)))
            .current_dir(&places.root)
            .status()
            .map_err(|e| format!("moteur {name} : construction impossible ({program}) : {e}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!(
                "moteur {name} : la construction a échoué ({status})"
            ))
        }
    }

    /// The command that runs the engine, from the root.
    pub fn command(&self, places: &Places) -> Command {
        let mut words = self.command.iter().map(|word| places.expand(word));
        let mut command = Command::new(words.next().unwrap_or_default());
        command.args(words).current_dir(&places.root);
        command
    }
}

/// Who answered, from its [`Reply::Engine`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EngineInfo {
    /// Its name, as it gives it.
    pub name: String,
    /// Its version, as it gives it.
    pub version: String,
    /// The rest of what it says about itself.
    pub detail: String,
}

/// What became of a requested page.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum PageOutcome {
    /// Drawn, encoded and written, as [`Reply::Page`] says.
    Rendered {
        /// Width of the image, in pixels.
        width: u32,
        /// Height of the image, in pixels.
        height: u32,
        /// Each time taken to draw it, in milliseconds.
        render_ms: Vec<f64>,
        /// Each time taken to encode it, in milliseconds.
        encode_ms: Vec<f64>,
        /// Whether the repetitions gave the same pixels.
        identical_repeats: bool,
    },
    /// No image, and why.
    Failed {
        /// In words.
        error: String,
    },
}

/// What an engine did with one document.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DocumentRun {
    /// Its presentation, when it gave one.
    pub engine: Option<EngineInfo>,
    /// Time to open the document, in milliseconds, when it opened.
    pub open_ms: Option<f64>,
    /// Each requested page, in the order of the request.
    pub pages: Vec<PageOutcome>,
    /// The end of what the engine wrote on its standard error.
    pub stderr: String,
    /// How the process ended.
    pub exit: String,
    /// Wall time of the process, from its start to its end, in milliseconds.
    pub wall_ms: f64,
}

/// Kibibytes of standard error kept for the report.
const STDERR_TAIL: usize = 16 * 1024;

/// Run `command` on `request`. The engine is stopped when `timeout` passes
/// without a line from it; the pages it did not answer then fail with the
/// reason, as they do when it crashes or breaks the protocol.
pub fn run_document(mut command: Command, request: &Request, timeout: Duration) -> DocumentRun {
    let started = Instant::now();
    let mut run = DocumentRun {
        engine: None,
        open_ms: None,
        pages: Vec::new(),
        stderr: String::new(),
        exit: String::new(),
        wall_ms: 0.0,
    };
    let spawned = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(e) => {
            let error = format!(
                "moteur impossible à lancer ({}) : {e}",
                command.get_program().to_string_lossy()
            );
            run.pages = vec![
                PageOutcome::Failed {
                    error: error.clone()
                };
                request.pages.len()
            ];
            run.exit = error;
            return run;
        }
    };
    // Both outputs are read on their own threads, from the start: an engine
    // never blocks on a full pipe, and a silent one cannot block the bench.
    let stderr = child
        .stderr
        .take()
        .map(|pipe| thread::spawn(move || read_tail(pipe, STDERR_TAIL)));
    let (sender, lines) = mpsc::channel();
    if let Some(stdout) = child.stdout.take() {
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let failed = line.is_err();
                if sender.send(line.map_err(|e| e.to_string())).is_err() || failed {
                    break;
                }
            }
        });
    }
    if let Some(mut stdin) = child.stdin.take() {
        // An engine that dies before reading closes the pipe: its missing
        // answer says so below.
        let text = serde_json::to_string(request).unwrap_or_default();
        let _ = stdin.write_all(text.as_bytes());
    }

    let mut pages: Vec<Option<PageOutcome>> = vec![None; request.pages.len()];
    let mut state = State::default();
    let stopped = loop {
        if pages.iter().all(Option::is_some) && state.opened {
            break None;
        }
        match lines.recv_timeout(timeout) {
            Ok(Ok(line)) if line.trim().is_empty() => {}
            Ok(Ok(line)) => {
                let taken = match serde_json::from_str::<Reply>(&line) {
                    Ok(reply) => take(reply, request, &mut state, &mut run, &mut pages),
                    Err(e) => Err(format!(
                        "réponse illisible du moteur ({e}) : {}",
                        shorten(&line, 200)
                    )),
                };
                if let Err(reason) = taken {
                    break Some(reason);
                }
            }
            Ok(Err(e)) => break Some(format!("sortie du moteur illisible : {e}")),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                break Some(format!(
                    "délai dépassé : aucune réponse du moteur pendant {} s",
                    timeout.as_secs()
                ))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break None,
        }
    };
    if stopped.is_some() {
        let _ = child.kill();
    }
    run.exit = wait(&mut child, timeout);
    run.wall_ms = started.elapsed().as_secs_f64() * 1000.0;
    run.stderr = stderr
        .and_then(|reader| reader.join().ok())
        .unwrap_or_default();
    let reason = stopped.unwrap_or_else(|| {
        let last = run.stderr.lines().rev().find(|l| !l.trim().is_empty());
        match last {
            Some(line) => format!(
                "le moteur s'est arrêté sans répondre ({}) : {}",
                run.exit,
                shorten(line, 300)
            ),
            None => format!("le moteur s'est arrêté sans répondre ({})", run.exit),
        }
    });
    run.pages = pages
        .into_iter()
        .map(|page| {
            page.unwrap_or_else(|| PageOutcome::Failed {
                error: reason.clone(),
            })
        })
        .collect();
    run
}

/// Where an engine is in the protocol.
#[derive(Debug, Default)]
struct State {
    introduced: bool,
    opened: bool,
}

/// Take one reply. `Err` ends the run of the document, and says what to
/// report for the pages still without an answer.
fn take(
    reply: Reply,
    request: &Request,
    state: &mut State,
    run: &mut DocumentRun,
    pages: &mut [Option<PageOutcome>],
) -> Result<(), String> {
    if !state.introduced && !matches!(reply, Reply::Engine { .. } | Reply::Fatal { .. }) {
        return Err("le moteur ne s'est pas présenté avant de répondre".to_string());
    }
    let mut answer = |index: usize, outcome: PageOutcome| {
        if !state.opened {
            return Err(format!(
                "le moteur répond pour la page {index} avant d'avoir ouvert le document"
            ));
        }
        let position = request
            .pages
            .iter()
            .position(|page| page.index == index)
            .ok_or_else(|| format!("le moteur répond pour la page {index}, non demandée"))?;
        match pages.get_mut(position) {
            Some(slot @ None) => {
                *slot = Some(outcome);
                Ok(())
            }
            _ => Err(format!("le moteur répond deux fois pour la page {index}")),
        }
    };
    match reply {
        Reply::Engine {
            protocol,
            name,
            version,
            detail,
        } => {
            if state.introduced {
                return Err("le moteur s'est présenté deux fois".to_string());
            }
            state.introduced = true;
            run.engine = Some(EngineInfo {
                name,
                version,
                detail,
            });
            if protocol != PROTOCOL {
                return Err(format!(
                    "le moteur parle le protocole {protocol}, le banc le protocole {PROTOCOL}"
                ));
            }
            Ok(())
        }
        Reply::Opened { ms } => {
            if state.opened {
                return Err("le moteur a ouvert le document deux fois".to_string());
            }
            state.opened = true;
            run.open_ms = Some(ms);
            Ok(())
        }
        Reply::OpenFailed { error } => Err(format!("document impossible à ouvrir : {error}")),
        Reply::Page {
            index,
            width,
            height,
            render_ms,
            encode_ms,
            identical_repeats,
        } => {
            if render_ms.len() != request.repeat as usize
                || encode_ms.len() != request.repeat as usize
            {
                return Err(format!(
                    "le moteur donne {} temps de rendu et {} d'encodage pour la page {index}, {} répétitions demandées",
                    render_ms.len(),
                    encode_ms.len(),
                    request.repeat
                ));
            }
            answer(
                index,
                PageOutcome::Rendered {
                    width,
                    height,
                    render_ms,
                    encode_ms,
                    identical_repeats,
                },
            )
        }
        Reply::PageFailed { index, error } => answer(index, PageOutcome::Failed { error }),
        Reply::Fatal { error } => Err(format!("le moteur s'est arrêté : {error}")),
    }
}

/// Wait for the end of `child`, stopping it after `timeout`; how it ended,
/// in words.
fn wait(child: &mut Child, timeout: Duration) -> String {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.to_string(),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(5)),
            Ok(None) => {
                let _ = child.kill();
                return match child.wait() {
                    Ok(status) => format!("{status}, arrêté par le banc"),
                    Err(e) => format!("arrêté par le banc : {e}"),
                };
            }
            Err(e) => return format!("fin inconnue : {e}"),
        }
    }
}

/// Everything `pipe` gives, the last `keep` bytes of it, as text.
fn read_tail(mut pipe: impl Read, keep: usize) -> String {
    let mut kept = Vec::new();
    let mut chunk = [0u8; 8192];
    while let Ok(read) = pipe.read(&mut chunk) {
        if read == 0 {
            break;
        }
        kept.extend_from_slice(&chunk[..read]);
        if kept.len() > 2 * keep {
            kept.drain(..kept.len() - keep);
        }
    }
    if kept.len() > keep {
        kept.drain(..kept.len() - keep);
    }
    String::from_utf8_lossy(&kept).into_owned()
}

/// `text` cut to `max` characters.
pub fn shorten(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        format!("{}…", text.chars().take(max).collect::<String>())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn engines_are_read_with_their_placeholders() {
        let engines = parse_engines(
            r#"
[pdfium]
description = "PDFium"
build = ["cargo", "build", "-p", "x"]
command = ["{bin_dir}/engine", "--root", "{root}"]
"#,
        )
        .unwrap();
        let places = Places {
            root: PathBuf::from("/repo"),
            bin_dir: PathBuf::from("/repo/target/release"),
        };
        let command = engines["pdfium"].command(&places);
        assert_eq!(command.get_program(), "/repo/target/release/engine");
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(args, ["--root", "/repo"]);
        assert_eq!(command.get_current_dir(), Some(Path::new("/repo")));
        for bad in [
            "[Pdfium]\ndescription = \"\"\ncommand = [\"x\"]\n",
            "[a]\ndescription = \"\"\ncommand = []\n",
            "[a]\ndescription = \"\"\ncommand = [\"x\"]\nbuilds = []\n",
        ] {
            assert!(parse_engines(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_missing_program_fails_every_page() {
        let request = Request {
            protocol: PROTOCOL,
            document: PathBuf::from("a.pdf"),
            password: String::new(),
            width: 100,
            repeat: 1,
            pages: vec![
                crate::protocol::PageRequest {
                    index: 0,
                    output: PathBuf::from("0.png"),
                };
                2
            ],
        };
        let run = run_document(
            Command::new("fyp-render-bench-no-such-engine"),
            &request,
            Duration::from_secs(5),
        );
        assert_eq!(run.pages.len(), 2);
        assert!(run
            .pages
            .iter()
            .all(|page| matches!(page, PageOutcome::Failed { error } if error.contains("impossible à lancer"))));
    }

    #[test]
    fn tails_and_shortening() {
        assert_eq!(read_tail(&b"abcdef"[..], 4), "cdef");
        assert_eq!(read_tail(&b"ab"[..], 4), "ab");
        assert_eq!(shorten("abcdef", 3), "abc…");
        assert_eq!(shorten("abc", 3), "abc");
    }
}
