//! `fyp` — the 4YouPDF command line.
//!
//! Milestone 0.1 commands: `info`, `rewrite`, `modules`. `merge` follows
//! the module host.

#![forbid(unsafe_code)]

use anyhow::Context;
use clap::{Parser, Subcommand};
use fyp_core::document::Document;
use fyp_core::encryption::{Cipher, Encryption};
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

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
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
