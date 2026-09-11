//! `fyp` — the 4YouPDF command line.
//!
//! Milestone 0.1 commands: `info`, `modules`. `rewrite` and `merge` follow
//! the document layer.

#![forbid(unsafe_code)]

use anyhow::Context;
use clap::{Parser, Subcommand};
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
            println!("version      PDF {}", info.version);
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
            // read (xref stream, broken table): report instead of failing.
            match fyp_core::document::Document::open(&bytes) {
                Ok(doc) => {
                    println!("objets       {} dans la xref", doc.xref().object_count());
                    match doc.page_count() {
                        Ok(n) => println!("pages        {n}"),
                        Err(e) => println!("pages        illisible ({e})"),
                    }
                }
                Err(e) => println!("structure    illisible ({e})"),
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
