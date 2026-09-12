//! `fyp` — the 4YouPDF command line.
//!
//! Milestone 0.1 commands: `info`, `rewrite`, `modules`. `merge` follows
//! the module host.

#![forbid(unsafe_code)]

use anyhow::Context;
use clap::{Parser, Subcommand};
use fyp_core::document::Document;
use fyp_core::writer::{Writer, XrefStyle};
use fyp_core::xref::SectionKind;
use std::path::PathBuf;

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

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Cmd::Info { path } => {
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
            println!(
                "chiffré      {}",
                if info.looks_encrypted {
                    "probablement"
                } else {
                    "non"
                }
            );
            println!(
                "%%EOF        {}",
                if info.has_eof_marker {
                    "présent"
                } else {
                    "absent"
                }
            );
            // The quick facts above stay useful when the structure cannot be
            // read (broken table, unsupported filter): report instead of failing.
            match fyp_core::document::Document::open(&bytes) {
                Ok(doc) => {
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
                Err(e) => println!("structure    illisible ({e})"),
            }
        }
        Cmd::Rewrite {
            input,
            output,
            xref_stream,
        } => {
            let bytes =
                std::fs::read(&input).with_context(|| format!("reading {}", input.display()))?;
            let doc =
                Document::open(&bytes).map_err(|e| anyhow::anyhow!("{}: {e}", input.display()))?;
            if let Some(reason) = doc.reconstructed() {
                println!("xref d'entrée reconstruite par scan ({reason})");
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
                eprintln!("refusé: {e}");
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
