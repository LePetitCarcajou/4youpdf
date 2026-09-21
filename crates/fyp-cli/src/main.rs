//! `fyp` — the 4YouPDF command line.
//!
//! Milestone 0.1 commands: `info`, `rewrite`, `modules`. Milestone 0.2:
//! `merge`, `pages extract|delete|rotate`, `split`, on `fyp_core::ops`;
//! `run`, which hands an action to a module in the WebAssembly sandbox.

#![forbid(unsafe_code)]

use std::borrow::Cow;
use std::collections::BTreeMap;

use anyhow::Context;
use clap::{Parser, Subcommand};
use fyp_core::document::Document;
use fyp_core::encryption::{Cipher, Encryption};
use fyp_core::ops;
use fyp_core::writer::{Writer, XrefStyle};
use fyp_core::xref::SectionKind;
use fyp_host::{HostError, ParamValue};
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(name = "fyp", version, about = "4YouPDF — le VLC du PDF")]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Show facts about a PDF: header, cross-reference table, page count
    Info {
        /// Path to the PDF
        path: PathBuf,
        /// User or owner password of an encrypted file (empty by default)
        #[arg(long, default_value = "")]
        password: String,
    },
    /// Rewrite a PDF as a clean single-section file: objects unpacked from
    /// object streams, exact stream lengths, regenerated cross-reference
    /// table. Repairs a broken table on the way.
    Rewrite {
        /// PDF to read, sound or broken
        input: PathBuf,
        /// File to write
        output: PathBuf,
        /// Write a cross-reference stream (PDF 1.5) instead of a classic table
        #[arg(long)]
        xref_stream: bool,
        /// User or owner password of an encrypted input (empty by default).
        /// The output is always written in the clear.
        #[arg(long, default_value = "")]
        password: String,
    },
    /// Concatenate several PDFs into one: `fyp merge a.pdf b.pdf -o c.pdf`
    Merge {
        /// PDFs to concatenate, in order
        #[arg(required = true)]
        inputs: Vec<PathBuf>,
        /// Pages to take from one input, 1-based, e.g. `1,3,5-8`, `8-5`
        /// for reverse order, a page twice to repeat it, or `all`. Give it
        /// once per input, in the same order; without it every input gives
        /// all its pages.
        #[arg(long = "pages", value_name = "PAGES")]
        pages: Vec<String>,
        /// File to write
        #[arg(short, long)]
        output: PathBuf,
        /// Password of the encrypted inputs, if any (the output is in the clear)
        #[arg(long, default_value = "")]
        password: String,
    },
    /// Keep, remove or turn pages: `fyp pages extract in.pdf 1,3,5-8 -o out.pdf`
    Pages {
        #[command(subcommand)]
        op: PagesCmd,
    },
    /// Cut a PDF into several files: `fyp split in.pdf --every 10 -o dir/`
    Split {
        /// PDF to cut
        input: PathBuf,
        /// Pages per part (the last part may be shorter)
        #[arg(long, conflicts_with = "ranges")]
        every: Option<usize>,
        /// Explicit parts, e.g. `1-3,4-6,7`; parts may overlap
        #[arg(long)]
        ranges: Option<String>,
        /// Directory to write the parts into (created if missing)
        #[arg(short, long)]
        output: PathBuf,
        /// Password of an encrypted input (the parts are in the clear)
        #[arg(long, default_value = "")]
        password: String,
    },
    /// Run a module's action in the WebAssembly sandbox:
    /// `fyp run merge a.pdf b.pdf -o c.pdf`. The module's result is
    /// re-validated and rewritten by the core before it is written.
    Run {
        /// Action to run, as declared in a module's manifest (e.g. `merge`)
        action: String,
        /// Documents handed to the module, in order
        inputs: Vec<PathBuf>,
        /// File to write
        #[arg(short, long)]
        output: PathBuf,
        /// Parameter of the action, `name=value`; repeat for several
        #[arg(long = "param", value_name = "NAME=VALUE")]
        params: Vec<String>,
        /// Directory containing one sub-directory per module
        #[arg(long, default_value = "plugins")]
        modules: PathBuf,
        /// Module to use when several declare the action, e.g. `org.fouryoupdf.merge`
        #[arg(long)]
        module: Option<String>,
        /// Password of the encrypted inputs: the host deciphers them and
        /// the module never sees it (the output is in the clear)
        #[arg(long, default_value = "")]
        password: String,
    },
    /// List modules found in a directory and why some are refused
    Modules {
        /// Directory containing one sub-directory per module
        #[arg(default_value = "plugins")]
        dir: PathBuf,
        /// Treat modules as trusted (in-repo). Never use for downloaded modules.
        #[arg(long)]
        trusted: bool,
    },
}

/// One line about how a file is protected, e.g. `révision 4, AES-128, clé 128 bits`.
fn describe_encryption(e: &Encryption) -> String {
    let cipher = |c: Cipher| match c {
        Cipher::Identity => "en clair",
        Cipher::Rc4 => "RC4",
        Cipher::Aes128 => "AES-128",
        Cipher::Aes256 => "AES-256",
    };
    let ciphers = if e.streams == e.strings {
        cipher(e.streams).to_string()
    } else {
        format!("flux {}, chaînes {}", cipher(e.streams), cipher(e.strings))
    };
    let mut text = format!(
        "révision {}, {ciphers}, clé {} bits",
        e.revision.number(),
        e.key_bits
    );
    if !e.encrypt_metadata {
        text.push_str(", métadonnées en clair");
    }
    if e.owner {
        text.push_str(", ouvert avec le mot de passe propriétaire");
    }
    text
}

#[derive(Subcommand)]
enum PagesCmd {
    /// Keep only these pages, in the order given (reordering allowed)
    Extract {
        /// PDF to read
        input: PathBuf,
        /// Pages to keep, 1-based, e.g. `1,3,5-8` or `8-5` for reverse order
        pages: String,
        /// File to write
        #[arg(short, long)]
        output: PathBuf,
        /// Password of an encrypted input (the output is in the clear)
        #[arg(long, default_value = "")]
        password: String,
    },
    /// Remove these pages
    Delete {
        /// PDF to read
        input: PathBuf,
        /// Pages to remove, 1-based, e.g. `2,4-6`
        pages: String,
        /// File to write
        #[arg(short, long)]
        output: PathBuf,
        /// Password of an encrypted input (the output is in the clear)
        #[arg(long, default_value = "")]
        password: String,
    },
    /// Turn these pages clockwise, relative to their current rotation
    Rotate {
        /// PDF to read
        input: PathBuf,
        /// Pages to turn, 1-based, e.g. `2,4`
        pages: String,
        /// Degrees, a multiple of 90 (negative turns counter-clockwise)
        #[arg(long)]
        degrees: i32,
        /// File to write
        #[arg(short, long)]
        output: PathBuf,
        /// Password of an encrypted input (the output is in the clear)
        #[arg(long, default_value = "")]
        password: String,
    },
}

/// Read a file for an operation; the bytes outlive the document.
fn read_input(path: &Path) -> anyhow::Result<Vec<u8>> {
    std::fs::read(path).with_context(|| format!("lecture de {}", path.display()))
}

/// Open an input for an operation and say when it is encrypted: the
/// output of every operation is in the clear.
fn open_input<'a>(path: &Path, bytes: &'a [u8], password: &str) -> anyhow::Result<Document<'a>> {
    let doc = Document::open_with_password(bytes, password.as_bytes()).map_err(|e| match e {
        fyp_core::Error::WrongPassword => anyhow::anyhow!(
            "{}: fichier chiffré, le mot de passe donné ne l'ouvre pas (--password)",
            path.display()
        ),
        e => anyhow::anyhow!("{}: {e}", path.display()),
    })?;
    if let Some(reason) = doc.reconstructed() {
        println!("{}: xref reconstruite par scan ({reason})", path.display());
    }
    if let Some(e) = doc.encryption() {
        println!(
            "{}: entrée chiffrée ({}) : la sortie est écrite EN CLAIR, sans aucune protection",
            path.display(),
            describe_encryption(&e)
        );
    }
    Ok(doc)
}

/// Parse a 1-based page list such as `1,3,5-8` or `8-5` into 0-based
/// indices, in the order written. Every number must be within `count`.
fn parse_pages(spec: &str, count: usize) -> anyhow::Result<Vec<usize>> {
    let number = |text: &str| -> anyhow::Result<usize> {
        let n: usize = text
            .trim()
            .parse()
            .map_err(|_| anyhow::anyhow!("numéro de page invalide : « {} »", text.trim()))?;
        if n == 0 || n > count {
            anyhow::bail!(
                "page {n} hors limites : le document a {count} page{}, numérotées de 1 à {count}",
                if count > 1 { "s" } else { "" }
            );
        }
        Ok(n - 1)
    };
    let mut pages = Vec::new();
    for part in spec.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        match part.split_once('-') {
            Some((from, to)) => {
                let (from, to) = (number(from)?, number(to)?);
                if from <= to {
                    pages.extend(from..=to);
                } else {
                    pages.extend((to..=from).rev());
                }
            }
            None => pages.push(number(part)?),
        }
    }
    if pages.is_empty() {
        anyhow::bail!("aucune page indiquée (exemple : 1,3,5-8)");
    }
    Ok(pages)
}

/// A `--pages` value of `fyp merge`, for one input: `all` for every page
/// of that file, or a 1-based page list such as `1,3,5-8` (see
/// [`parse_pages`], which a merge lets repeat a page).
fn parse_selection(path: &Path, count: usize, spec: &str) -> anyhow::Result<ops::Selection> {
    let spec = spec.trim();
    if spec.eq_ignore_ascii_case("all") {
        return Ok(ops::Selection::All);
    }
    if spec.is_empty() {
        anyhow::bail!(
            "{}: aucune page indiquée (exemple : 1,3,5-8, ou « all » pour tout le fichier)",
            path.display()
        );
    }
    parse_pages(spec, count)
        .map(ops::Selection::Pages)
        .map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))
}

/// Parse `1-3,4-6,7` into 0-based ranges, end excluded.
fn parse_ranges(spec: &str, count: usize) -> anyhow::Result<Vec<std::ops::Range<usize>>> {
    let mut ranges = Vec::new();
    for part in spec.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let pages = parse_pages(part, count)?;
        let (first, last) = (pages[0], pages[pages.len() - 1]);
        if first > last {
            anyhow::bail!("plage à l'envers : « {part} »");
        }
        ranges.push(first..last + 1);
    }
    if ranges.is_empty() {
        anyhow::bail!("aucune plage indiquée (exemple : 1-3,4-6)");
    }
    Ok(ranges)
}

/// Write an operation's result, read it back, and report.
fn write_result(output: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    std::fs::write(output, bytes).with_context(|| format!("écriture de {}", output.display()))?;
    let check = Document::open(bytes)
        .map_err(|e| anyhow::anyhow!("{}: relecture impossible: {e}", output.display()))?;
    if let Some(reason) = check.reconstructed() {
        anyhow::bail!(
            "{}: le fichier écrit a dû être réparé à la relecture ({reason})",
            output.display()
        );
    }
    println!(
        "écrit        {} ({} octets, PDF {}, {} page{})",
        output.display(),
        bytes.len(),
        check.version(),
        check.page_count().unwrap_or(0),
        if check.page_count().unwrap_or(0) > 1 {
            "s"
        } else {
            ""
        }
    );
    Ok(())
}

fn page_count(path: &Path, doc: &Document<'_>) -> anyhow::Result<usize> {
    ops::pages(doc)
        .map(|p| p.len())
        .map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))
}

/// A host error as the user reads it: the host's message and, for the
/// refusals the user can act on, what to change.
fn explain(e: &HostError) -> String {
    let hint = match e {
        HostError::LimitAboveCeiling { limit, ceiling, .. } => {
            let unit = if *limit == "timeout_ms" { "ms" } else { "Mio" };
            format!(
                "ce module demande plus que ce que l'hôte accorde : `{limit}` doit valoir au plus {ceiling} {unit} dans son manifest.toml"
            )
        }
        HostError::HostMemoryExhausted { budget_mib, .. } => format!(
            "la mémoire du module, sa réponse et la re-validation de celle-ci ont dépassé les {budget_mib} Mio que l'hôte accorde à l'ensemble des modules"
        ),
        HostError::DuplicateId { id, .. } => format!(
            "tant que plusieurs dossiers déclarent `{id}`, aucun n'est chargé : retirer les copies en trop, ou changer l'`id` de leur manifest.toml"
        ),
        _ => return e.to_string(),
    };
    format!("{e}\n({hint})")
}

/// `fyp run`: find the module declaring `action`, hand it the inputs in
/// its sandbox, write the re-validated result.
fn run_action(
    action: &str,
    inputs: &[PathBuf],
    output: &Path,
    params: &[String],
    modules: &Path,
    module: Option<&str>,
    password: &str,
) -> anyhow::Result<()> {
    let (found, refused) = fyp_host::discover(modules, false);
    let candidates: Vec<&fyp_host::DiscoveredModule> = found
        .iter()
        .filter(|m| module.is_none_or(|id| m.manifest.id == id))
        .filter(|m| m.manifest.actions.iter().any(|a| a.id == action))
        .collect();
    let chosen = match candidates.as_slice() {
        [one] => *one,
        [] => {
            for e in &refused {
                eprintln!("refusé: {}", explain(e));
            }
            anyhow::bail!(
                "aucun module accepté dans {} ne déclare l'action « {action} »",
                modules.display()
            )
        }
        several => anyhow::bail!(
            "plusieurs modules déclarent l'action « {action} » : {} (préciser --module)",
            several
                .iter()
                .map(|m| m.manifest.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    };
    let declared = chosen
        .manifest
        .actions
        .iter()
        .find(|a| a.id == action)
        .map(|a| a.params.as_slice())
        .unwrap_or_default();
    let mut values = BTreeMap::new();
    for param in params {
        let Some((name, text)) = param.split_once('=') else {
            anyhow::bail!("paramètre « {param} » : écrire nom=valeur");
        };
        let Some(spec) = declared.iter().find(|p| p.id == name) else {
            anyhow::bail!("l'action « {action} » n'a pas de paramètre « {name} »");
        };
        let value = ParamValue::parse(spec.kind, text)
            .map_err(|e| anyhow::anyhow!("paramètre « {name} » : {e}"))?;
        values.insert(name.to_string(), value);
    }

    let files: Vec<Vec<u8>> = inputs
        .iter()
        .map(|p| read_input(p))
        .collect::<anyhow::Result<_>>()?;
    let mut documents: Vec<Cow<'_, [u8]>> = Vec::with_capacity(files.len());
    for (path, bytes) in inputs.iter().zip(&files) {
        let doc = open_input(path, bytes, password)?;
        // The module never receives a password: an encrypted input is
        // deciphered here and handed over in the clear.
        if doc.encryption().is_some() {
            let clear = Writer::new(doc.version())
                .write(&doc)
                .map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
            documents.push(Cow::Owned(clear));
        } else {
            documents.push(Cow::Borrowed(bytes));
        }
    }

    let host = fyp_host::Host::new().map_err(|e| anyhow::anyhow!("{e}"))?;
    let loaded = host.load(chosen).map_err(|e| match e {
        HostError::Io { .. } => anyhow::anyhow!(
            "{e}\n(les modules du dépôt se construisent avec : python tools/build_modules.py)"
        ),
        e => anyhow::anyhow!("{}", explain(&e)),
    })?;
    let permissions: Vec<String> = chosen
        .manifest
        .permissions
        .iter()
        .map(|p| format!("{p:?}"))
        .collect();
    println!(
        "module       {} {} (sandbox WebAssembly, permissions [{}])",
        chosen.manifest.id,
        chosen.manifest.version,
        permissions.join(", ")
    );
    let refs: Vec<&[u8]> = documents.iter().map(|d| d.as_ref()).collect();
    let result = loaded
        .run(action, &values, &refs)
        .map_err(|e| anyhow::anyhow!("{}", explain(&e)))?;
    if !result.diagnostics.trim().is_empty() {
        eprintln!("module (stderr) : {}", result.diagnostics.trim_end());
    }
    match &result.reconstructed {
        Some(reason) => println!(
            "revalidé     document du module reconstruit ({reason}), puis réécrit par le noyau"
        ),
        None => println!("revalidé     document du module relu et réécrit par le noyau"),
    }
    write_result(output, &result.document)
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Cmd::Merge {
            inputs,
            pages,
            output,
            password,
        } => {
            if !pages.is_empty() && pages.len() != inputs.len() {
                let plural = |n: usize| if n > 1 { "s" } else { "" };
                anyhow::bail!(
                    "--pages : une sélection par fichier, dans le même ordre ({} fichier{}, {} sélection{}) ; « all » prend tout un fichier",
                    inputs.len(),
                    plural(inputs.len()),
                    pages.len(),
                    plural(pages.len())
                );
            }
            let files: Vec<Vec<u8>> = inputs
                .iter()
                .map(|p| read_input(p))
                .collect::<anyhow::Result<_>>()?;
            let docs: Vec<Document<'_>> = inputs
                .iter()
                .zip(&files)
                .map(|(path, bytes)| open_input(path, bytes, &password))
                .collect::<anyhow::Result<_>>()?;
            let selections: Vec<ops::Selection> = if pages.is_empty() {
                vec![ops::Selection::All; docs.len()]
            } else {
                inputs
                    .iter()
                    .zip(&docs)
                    .zip(&pages)
                    .map(|((path, doc), spec)| parse_selection(path, page_count(path, doc)?, spec))
                    .collect::<anyhow::Result<_>>()?
            };
            let out = ops::merge_selected(&docs, &selections)
                .map_err(|e| anyhow::anyhow!("fusion : {e}"))?;
            write_result(&output, &out)?;
        }
        Cmd::Pages { op } => match op {
            PagesCmd::Extract {
                input,
                pages,
                output,
                password,
            } => {
                let bytes = read_input(&input)?;
                let doc = open_input(&input, &bytes, &password)?;
                let indices = parse_pages(&pages, page_count(&input, &doc)?)?;
                let out = ops::extract_pages(&doc, &indices)
                    .map_err(|e| anyhow::anyhow!("{}: {e}", input.display()))?;
                write_result(&output, &out)?;
            }
            PagesCmd::Delete {
                input,
                pages,
                output,
                password,
            } => {
                let bytes = read_input(&input)?;
                let doc = open_input(&input, &bytes, &password)?;
                let indices = parse_pages(&pages, page_count(&input, &doc)?)?;
                let out = ops::delete_pages(&doc, &indices)
                    .map_err(|e| anyhow::anyhow!("{}: {e}", input.display()))?;
                write_result(&output, &out)?;
            }
            PagesCmd::Rotate {
                input,
                pages,
                degrees,
                output,
                password,
            } => {
                let bytes = read_input(&input)?;
                let doc = open_input(&input, &bytes, &password)?;
                let indices = parse_pages(&pages, page_count(&input, &doc)?)?;
                let out = ops::rotate(&doc, &indices, degrees)
                    .map_err(|e| anyhow::anyhow!("{}: {e}", input.display()))?;
                write_result(&output, &out)?;
            }
        },
        Cmd::Split {
            input,
            every,
            ranges,
            output,
            password,
        } => {
            let bytes = read_input(&input)?;
            let doc = open_input(&input, &bytes, &password)?;
            let count = page_count(&input, &doc)?;
            let ranges = match (every, ranges) {
                (Some(n), _) if n > 0 => ops::ranges_every(count, n),
                (Some(_), _) => anyhow::bail!("--every doit être au moins 1"),
                (None, Some(spec)) => parse_ranges(&spec, count)?,
                (None, None) => anyhow::bail!("indiquer --every N ou --ranges 1-3,4-6"),
            };
            std::fs::create_dir_all(&output)
                .with_context(|| format!("création de {}", output.display()))?;
            let parts = ops::split(&doc, &ranges)
                .map_err(|e| anyhow::anyhow!("{}: {e}", input.display()))?;
            let stem = input
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "partie".into());
            for (part, range) in parts.iter().zip(&ranges) {
                let name = format!("{stem}-{:03}-{:03}.pdf", range.start + 1, range.end);
                write_result(&output.join(name), part)?;
            }
            println!("{} fichier(s) dans {}", parts.len(), output.display());
        }
        Cmd::Info { path, password } => {
            let bytes =
                std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
            let info = fyp_core::version::quick_info(&bytes)
                .map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
            println!("fichier      {}", path.display());
            println!("taille       {} octets", bytes.len());
            if info.header_present {
                println!("version      PDF {}", info.version);
            } else {
                println!(
                    "version      PDF {} (supposée : en-tête %PDF absent)",
                    info.version
                );
            }
            if info.header_offset > 0 {
                println!(
                    "en-tête      décalé de {} octets (toléré)",
                    info.header_offset
                );
            }
            match info.startxref {
                Some(off) => println!("startxref    {off}"),
                None => println!("startxref    absent (reconstruction nécessaire)"),
            }
            // The quick facts stay useful when the structure cannot be read
            // (broken table, unsupported filter): report instead of failing.
            let opened = Document::open_with_password(&bytes, password.as_bytes());
            // The guess of `quick_info` (an `/Encrypt` near the end) is only
            // worth showing when the document itself could not answer.
            match &opened {
                Ok(doc) if doc.encryption().is_some() => {}
                Ok(_) => println!("chiffré      non"),
                Err(_) => println!(
                    "chiffré      {}",
                    if info.looks_encrypted {
                        "probablement"
                    } else {
                        "non"
                    }
                ),
            }
            println!(
                "%%EOF        {}",
                if info.has_eof_marker {
                    "présent"
                } else {
                    "absent"
                }
            );
            match opened {
                Ok(doc) => {
                    if let Some(e) = doc.encryption() {
                        println!("chiffrement  {}", describe_encryption(&e));
                    }
                    if let Some(at) = doc.relocated_startxref() {
                        println!(
                            "startxref    faux, table trouvée à l'offset {at} (toléré, sans reconstruction)"
                        );
                    }
                    // A repaired file must never pass for a sound one: say
                    // why the declared table was dropped.
                    match doc.reconstructed() {
                        Some(reason) => println!(
                            "xref         reconstruite par scan (table déclarée inutilisable : {reason})"
                        ),
                        None => {
                            // Kind of the newest section, the one `startxref` points to.
                            let kind = match doc.xref().kind() {
                                SectionKind::Table => "table classique",
                                SectionKind::Stream => "flux xref",
                                SectionKind::Hybrid => "hybride (table + /XRefStm)",
                                SectionKind::Reconstructed => "reconstruite par scan",
                            };
                            println!("section xref {kind}");
                        }
                    }
                    println!("objets       {} dans la xref", doc.xref().object_count());
                    match doc.page_count() {
                        Ok(n) => println!("pages        {n}"),
                        Err(e) => println!("pages        illisible ({e})"),
                    }
                }
                Err(fyp_core::Error::WrongPassword) => {
                    println!("structure    chiffrée, mot de passe requis (--password)")
                }
                Err(e) => println!("structure    illisible ({e})"),
            }
        }
        Cmd::Rewrite {
            input,
            output,
            xref_stream,
            password,
        } => {
            let bytes =
                std::fs::read(&input).with_context(|| format!("reading {}", input.display()))?;
            let doc =
                Document::open_with_password(&bytes, password.as_bytes()).map_err(|e| match e {
                    fyp_core::Error::WrongPassword => anyhow::anyhow!(
                        "{}: fichier chiffré, le mot de passe donné ne l'ouvre pas (--password)",
                        input.display()
                    ),
                    e => anyhow::anyhow!("{}: {e}", input.display()),
                })?;
            if let Some(reason) = doc.reconstructed() {
                println!("xref d'entrée reconstruite par scan ({reason})");
            }
            // The writer does not encrypt yet: say so, every time, before
            // the file lands on disk.
            if let Some(e) = doc.encryption() {
                println!(
                    "chiffrement  entrée chiffrée ({}) : la sortie est écrite EN CLAIR, sans aucune protection",
                    describe_encryption(&e)
                );
            }
            let style = if xref_stream {
                XrefStyle::Stream
            } else {
                XrefStyle::Table
            };
            let writer = Writer::new(doc.version()).xref_style(style);
            let out = writer
                .write(&doc)
                .map_err(|e| anyhow::anyhow!("{}: {e}", input.display()))?;
            std::fs::write(&output, &out)
                .with_context(|| format!("writing {}", output.display()))?;
            // Read the result back: the file must open without repair.
            let check = Document::open(&out)
                .map_err(|e| anyhow::anyhow!("{}: relecture impossible: {e}", output.display()))?;
            if let Some(reason) = check.reconstructed() {
                anyhow::bail!(
                    "{}: le fichier écrit a dû être réparé à la relecture ({reason})",
                    output.display()
                );
            }
            println!(
                "écrit        {} ({} octets, PDF {}, {})",
                output.display(),
                out.len(),
                writer.version(),
                match style {
                    XrefStyle::Table => "table classique",
                    XrefStyle::Stream => "flux xref",
                }
            );
            println!("objets       {}", check.xref().object_count());
            match check.page_count() {
                Ok(n) => println!("pages        {n}"),
                Err(e) => println!("pages        illisible ({e})"),
            }
        }
        Cmd::Run {
            action,
            inputs,
            output,
            params,
            modules,
            module,
            password,
        } => run_action(
            &action,
            &inputs,
            &output,
            &params,
            &modules,
            module.as_deref(),
            &password,
        )?,
        Cmd::Modules { dir, trusted } => {
            let (found, errors) = fyp_host::discover(&dir, trusted);
            for m in &found {
                let perms: Vec<String> = m
                    .manifest
                    .permissions
                    .iter()
                    .map(|p| format!("{p:?}"))
                    .collect();
                println!(
                    "{:<24} {:<12} {:<8} {:?} [{}]",
                    m.manifest.id,
                    m.manifest.version,
                    if m.trusted { "trusted" } else { "sandbox" },
                    m.manifest.runtime,
                    perms.join(", ")
                );
            }
            for e in &errors {
                eprintln!("refusé: {}", explain(e));
            }
            println!(
                "{} module(s) accepté(s), {} refusé(s)",
                found.len(),
                errors.len()
            );
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    /// `--pages` of `fyp merge`, for one input: the page-range syntax of
    /// the other commands, plus `all`, and a page a merge may repeat.
    #[test]
    fn merge_selections_are_parsed_and_refused_clearly() {
        let path = Path::new("rapport.pdf");
        let parse = |spec: &str| parse_selection(path, 12, spec);
        assert_eq!(parse("all").unwrap(), ops::Selection::All);
        assert_eq!(parse(" ALL ").unwrap(), ops::Selection::All);
        assert_eq!(
            parse("1,3,5-8").unwrap(),
            ops::Selection::Pages(vec![0, 2, 4, 5, 6, 7])
        );
        assert_eq!(
            parse("8-5").unwrap(),
            ops::Selection::Pages(vec![7, 6, 5, 4])
        );
        assert_eq!(parse("3,3").unwrap(), ops::Selection::Pages(vec![2, 2]));
        for (spec, expected) in [
            ("13", "hors limites"),
            ("0", "hors limites"),
            ("2-x", "numéro de page invalide"),
            ("", "aucune page indiquée"),
            ("  ", "aucune page indiquée"),
        ] {
            let message = parse(spec).unwrap_err().to_string();
            assert!(message.contains(expected), "{spec:?}: {message}");
            assert!(message.contains("rapport.pdf"), "{spec:?}: {message}");
        }
    }

    /// Each refusal names the module, the numbers involved and what to
    /// change. `HostMemoryExhausted` cannot be reached from `fyp run`
    /// without committing about 4 GiB: its wording is checked here only;
    /// `tests/run.rs` goes through the binary for the other two.
    #[test]
    fn host_refusals_say_what_is_wrong_and_what_to_change() {
        let cases: [(HostError, &[&str]); 3] = [
            (
                HostError::LimitAboveCeiling {
                    module: "org.example.greedy".into(),
                    limit: "memory_mib",
                    declared: 9000,
                    ceiling: 4096,
                },
                &[
                    "org.example.greedy",
                    "memory_mib = 9000",
                    "au plus 4096 Mio",
                    "manifest.toml",
                ],
            ),
            (
                HostError::HostMemoryExhausted {
                    module: "org.example.greedy".into(),
                    budget_mib: 4096,
                },
                &[
                    "org.example.greedy",
                    "4096 MiB",
                    "4096 Mio",
                    "re-validation",
                ],
            ),
            (
                HostError::DuplicateId {
                    id: "org.example.twin".into(),
                    dirs: vec!["plugins/twin-a".into(), "plugins/twin-b".into()],
                },
                &[
                    "org.example.twin",
                    "plugins/twin-a",
                    "plugins/twin-b",
                    "aucun n'est chargé",
                ],
            ),
        ];
        for (error, expected) in cases {
            let shown = explain(&error);
            for part in expected {
                assert!(shown.contains(part), "{part:?} missing from:\n{shown}");
            }
        }
    }
}
