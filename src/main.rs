//! `xapianbuilder` — CLI for building kiwix-compatible Xapian indexes.

use std::fs::File;
use std::io::{BufRead, BufReader, Read, stdin};
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use rayon::prelude::*;
use serde::Deserialize;

use xapianbuilder::{Builder, Mode, parse::ParsedDoc, stopwords};

/// Parallel-pipeline chunk size — the JSONL reader buffers up to
/// CHUNK lines, then dispatches them across worker threads. Larger
/// chunks amortise rayon's per-batch overhead; smaller chunks keep
/// memory use bounded for big bodies. 256 is a reasonable default for
/// Wikipedia-sized articles (~50 KB each → ~12 MB peak per chunk).
const CHUNK: usize = 256;

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
    /// Language string passed straight through to ICU (for stemmer
    /// selection) and used as the stopwords filename. libzim's
    /// convention is ISO-639-3 here ("eng", "fra"); pass the 2-letter
    /// code ("en", "fr") if you want stopword filtering, since the
    /// vendored files are named that way. Empty disables stemming and
    /// stopwords entirely.
    #[arg(long, default_value = "")]
    language: String,
    /// Path to a stopwords file. Overrides whatever `--language` would
    /// resolve to. One stopword per line.
    #[arg(long)]
    stopwords_file: Option<PathBuf>,
    /// Translate `--language` from ISO-639-3 to ISO-639-1 when looking
    /// up the vendored stopwords list (so `--language eng` actually
    /// loads `data/stopwords/en`). Default off — matches kiwix's
    /// canonical output, which silently has no stopword filtering
    /// because libzim's indexer does an exact filename match.
    #[arg(long)]
    resolve_stopwords: bool,
    /// Override the stemmer string passed to `Xapian::Stem`. Forwarded
    /// verbatim. Pass `porter` for the old (Porter1) stemmer used by
    /// pre-2024 kiwix ZIMs, `english` (or empty) for current Porter2,
    /// `none` to disable stemming. Empty falls back to the language
    /// derived from `--language` via ICU.
    #[arg(long, default_value = "")]
    stemmer: String,
    /// Skip writing the output file if zero docs were indexed (matches
    /// libzim, which omits `X/fulltext/xapian` for empty corpora).
    #[arg(long)]
    skip_if_empty: bool,
    /// Worker thread count. 0 (default) = num_cpus. 1 = strictly
    /// single-threaded. The HTML parser runs per-thread; database
    /// writes are serialised by an internal mutex inside the C++
    /// bridge, matching libzim's `s_dbaccessLock` design.
    #[arg(long, default_value_t = 0)]
    jobs: usize,
    /// Store per-doc termlists in the output DB. Default off — matches
    /// modern kiwix ZIMs (libzim sets DB_NO_TERMLIST). Turn this on
    /// when you need to walk terms per document (e.g. for
    /// `xapian-delve -1 -r N`); the resulting DB will be larger and
    /// won't byte-match a kiwix-built reference.
    #[arg(long)]
    keep_termlists: bool,
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

    let stopwords_owned;
    let stopwords_text: &str = if let Some(p) = &args.stopwords_file {
        stopwords_owned = std::fs::read_to_string(p)
            .with_context(|| format!("reading stopwords file {}", p.display()))?;
        &stopwords_owned
    } else if args.resolve_stopwords {
        stopwords::resolve_iso6393(&args.language)
    } else {
        stopwords::for_language(&args.language)
    };
    let builder = Builder::new(
        &tmp,
        &args.output,
        &args.language,
        stopwords_text,
        &args.stemmer,
        args.keep_termlists,
        mode,
    )?;

    if args.jobs > 0 {
        rayon::ThreadPoolBuilder::new()
            .num_threads(args.jobs)
            .build_global()
            .ok(); // already initialised on a second invocation; harmless.
    }

    let reader: Box<dyn Read> = if args.input == "-" {
        Box::new(stdin().lock())
    } else {
        Box::new(File::open(&args.input).with_context(|| {
            format!("opening input {}", &args.input)
        })?)
    };
    let reader = BufReader::with_capacity(1 << 20, reader);

    let mut chunk: Vec<String> = Vec::with_capacity(CHUNK);
    let mut count: u64 = 0;
    let flush = |chunk: &mut Vec<String>, count: &mut u64| -> Result<()> {
        if chunk.is_empty() {
            return Ok(());
        }
        // Parse in parallel — `par_iter().collect()` preserves input
        // order — then add to the DB serially. Doc IDs in Xapian are
        // assigned in `add_document` call order, so this keeps them
        // stable regardless of `--jobs`. The expensive work (HTML
        // strip, ICU lower+strip accents, word count) happens in the
        // workers; the serial leg only does a couple of CString
        // copies and the actual Xapian write.
        let prepared: Vec<Result<Prepared>> = match mode {
            Mode::Fulltext => chunk
                .par_iter()
                .map(|line| prepare_fulltext(line))
                .collect(),
            Mode::Title => chunk
                .par_iter()
                .map(|line| prepare_title(line))
                .collect(),
        };
        for res in prepared {
            match res? {
                Prepared::Skip => continue,
                Prepared::Title { path, title, target_path } => {
                    builder.add_title(&path, &title, &target_path)?;
                }
                Prepared::Fulltext {
                    path, title, content, keywords, word_count, geo,
                } => {
                    builder.add_fulltext(
                        &path, &title, &content, &keywords, word_count, geo,
                    )?;
                }
            }
        }
        *count += chunk.len() as u64;
        chunk.clear();
        Ok(())
    };

    for line in reader.lines() {
        let line = line.context("reading input")?;
        if line.trim().is_empty() {
            continue;
        }
        chunk.push(line);
        if chunk.len() >= CHUNK {
            flush(&mut chunk, &mut count)?;
            if !args.quiet && count.is_multiple_of(1000) {
                eprintln!("{count} docs indexed");
            }
        }
    }
    flush(&mut chunk, &mut count)?;

    if args.skip_if_empty && builder.is_empty() {
        if !args.quiet {
            eprintln!("done: 0 indexable docs, skipping {}", args.output.display());
        }
        // Drop runs the temp cleanup; we never call finalize() so no
        // output file is created. Done.
        return Ok(());
    }

    builder.finalize()?;

    if !args.quiet {
        eprintln!("done: {count} docs -> {}", args.output.display());
    }
    Ok(())
}

/// A document already parsed and ready to be handed to the Builder.
/// Computed in worker threads, applied serially.
enum Prepared {
    Skip,
    Title {
        path: String,
        title: String,
        target_path: String,
    },
    Fulltext {
        path: String,
        title: String,
        content: String,
        keywords: String,
        word_count: u32,
        geo: Option<(f64, f64)>,
    },
}

fn prepare_title(line: &str) -> Result<Prepared> {
    let doc: InputDoc = serde_json::from_str(line)
        .with_context(|| format!("parsing JSONL: {}", line.chars().take(80).collect::<String>()))?;
    Ok(Prepared::Title {
        path: doc.path,
        title: doc.title,
        target_path: doc.target_path.unwrap_or_default(),
    })
}

fn prepare_fulltext(line: &str) -> Result<Prepared> {
    let doc: InputDoc = serde_json::from_str(line)
        .with_context(|| format!("parsing JSONL: {}", line.chars().take(80).collect::<String>()))?;

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
            // Non-HTML mimetypes pass through verbatim. libzim only
            // runs MyHtmlParser, so anything else lands in the index
            // unmodified — no accent strip, no keyword extraction.
            let wc = doc.body.split_whitespace().count() as u32;
            (
                doc.body.clone(),
                String::new(),
                wc,
                None,
                !doc.body.is_empty(),
            )
        }
    };

    if !indexable {
        return Ok(Prepared::Skip);
    }
    Ok(Prepared::Fulltext {
        path: doc.path,
        title: doc.title,
        content,
        keywords,
        word_count,
        geo,
    })
}

fn with_suffix(p: &PathBuf, suffix: &str) -> PathBuf {
    let mut s = p.clone().into_os_string();
    s.push(suffix);
    PathBuf::from(s)
}
