//! `xapianbuilder` — CLI for building kiwix-compatible Xapian indexes.

use std::fs::File;
use std::io::{BufRead, BufReader, Read, stdin};
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use serde::Deserialize;

use xapianbuilder::{Builder, Mode, parse::ParsedDoc, stopwords};

#[derive(Parser, Debug)]
#[command(version, about = "Build kiwix-compatible Xapian indexes")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Build the X/fulltext/xapian database.
    Fulltext(BuildArgs),
    /// Build the X/title/xapian database.
    Title(BuildArgs),
    /// Print version.
    Version,
}

#[derive(Parser, Debug)]
struct BuildArgs {
    /// JSONL file with one document per line. Use "-" for stdin.
    #[arg(long)]
    input: String,
    /// Path to write the final single-file glass DB. The same path
    /// with a `.tmp` suffix is used as the WritableDatabase scratch
    /// directory and is removed on success.
    #[arg(long)]
    output: PathBuf,
    /// ISO-639-3 language for stemming/stopwords. The kiwix convention
    /// is one DB per language; "auto" or empty is treated as "no
    /// stemmer" (terms are still indexed unstemmed).
    #[arg(long, default_value = "")]
    language: String,
    /// Suppress progress output.
    #[arg(long)]
    quiet: bool,
}

#[derive(Debug, Deserialize)]
struct InputDoc {
    path: String,
    title: String,
    #[serde(default)]
    mimetype: String,
    #[serde(default)]
    body: String,
    /// Optional per-document override for the language selected at the
    /// CLI. If both are unset we skip stemming. Currently ignored (CLI
    /// language is authoritative) — accepted so callers can ship the
    /// field today.
    #[serde(default)]
    #[allow(dead_code)]
    language: Option<String>,
    /// Set on title-DB inputs that are redirects.
    #[serde(default)]
    target_path: Option<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Version => {
            println!("xapianbuilder {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Cmd::Title(args) => run(args, Mode::Title),
        Cmd::Fulltext(args) => run(args, Mode::Fulltext),
    }
}

fn run(args: BuildArgs, mode: Mode) -> Result<()> {
    if args.output.exists() {
        bail!(
            "output path already exists: {} (xapianbuilder refuses to overwrite)",
            args.output.display()
        );
    }
    let tmp = with_suffix(&args.output, ".tmp");
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp).ok();
    }

    let stopwords_text = stopwords::for_iso6393(&args.language);
    let mut builder = Builder::new(
        &tmp,
        &args.output,
        &args.language,
        stopwords_text,
        mode,
    )?;

    let reader: Box<dyn Read> = if args.input == "-" {
        Box::new(stdin().lock())
    } else {
        Box::new(File::open(&args.input).with_context(|| {
            format!("opening input {}", &args.input)
        })?)
    };
    let reader = BufReader::with_capacity(1 << 20, reader);

    let mut count: u64 = 0;
    for line in reader.lines() {
        let line = line.context("reading input")?;
        if line.trim().is_empty() {
            continue;
        }
        let doc: InputDoc = serde_json::from_str(&line)
            .with_context(|| format!("parsing JSONL line {}", count + 1))?;
        process_doc(&mut builder, mode, doc)?;
        count += 1;
        if !args.quiet && count.is_multiple_of(1000) {
            eprintln!("{count} docs indexed");
        }
    }

    builder.finalize()?;

    // libzim removes its `.tmp` directory in the indexer destructor;
    // we do it explicitly here on success.
    std::fs::remove_dir_all(&tmp).ok();

    if !args.quiet {
        eprintln!("done: {count} docs -> {}", args.output.display());
    }
    Ok(())
}

fn process_doc(builder: &mut Builder, mode: Mode, doc: InputDoc) -> Result<()> {
    match mode {
        Mode::Title => {
            let target = doc.target_path.as_deref().unwrap_or("");
            builder.add_title(&doc.path, &doc.title, target)
        }
        Mode::Fulltext => {
            let parsed = if doc.mimetype.starts_with("text/html") {
                ParsedDoc::parse(doc.body.as_bytes())
            } else {
                None
            };

            let (content, keywords, word_count, geo, indexable) = match &parsed {
                Some(p) => (
                    p.content().to_string(),
                    p.keywords().to_string(),
                    p.word_count(),
                    p.geo(),
                    p.indexing_allowed(),
                ),
                None => {
                    // text/plain or anything else: index body verbatim
                    // (still no accent-stripping; libzim only does it
                    // through MyHtmlParser, so plain text passes
                    // through unchanged, matching its behaviour).
                    let wc = doc.body.split_whitespace().count() as u32;
                    (doc.body.clone(), String::new(), wc, None, !doc.body.is_empty())
                }
            };

            if !indexable {
                return Ok(());
            }
            builder.add_fulltext(
                &doc.path,
                &doc.title,
                &content,
                &keywords,
                word_count,
                geo,
            )
        }
    }
}

fn with_suffix(p: &PathBuf, suffix: &str) -> PathBuf {
    let mut s = p.clone().into_os_string();
    s.push(suffix);
    PathBuf::from(s)
}
