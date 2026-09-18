//! `xapianbuilder` — CLI for building kiwix-compatible Xapian indexes.

use std::fs::File;
use std::io::{stdin, BufRead, BufReader, Lines, Read};
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use rayon::prelude::*;
use serde::Deserialize;

use xapianbuilder::{
    parse::ParsedDoc, stopwords, AccentRule, Builder, BuilderOptions, FulltextDocument, Mode,
    PreparedDoc,
};

/// Parallel-pipeline chunk size — the JSONL reader buffers up to
/// CHUNK lines, then dispatches them across worker threads. Larger
/// chunks amortise rayon's per-batch overhead; smaller chunks keep
/// memory use bounded for big bodies. 256 is a reasonable default for
/// Wikipedia-sized articles (~50 KB each → ~12 MB peak per chunk).
const CHUNK: usize = 256;

/// Byte budget for one in-flight chunk. Paired with CHUNK so that a few
/// very large entries cannot put an unbounded amount of content in flight;
/// see [`read_chunk`].
const CHUNK_BYTES: usize = 64 << 20;

/// Default value for `XAPIAN_FLUSH_THRESHOLD` (docs buffered in
/// memory between glass B-tree flushes) when neither `--flush-threshold`
/// nor the environment variable is set. Xapian's built-in default of
/// 10000 is tuned for incremental updates; bulk index builds go
/// noticeably faster with fewer, larger flushes. ~50k Wikipedia-sized
/// docs buffer a few hundred MB of postings.
const DEFAULT_FLUSH_THRESHOLD: &str = "50000";

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
    /// Final single-file glass DB. Published without overwriting an existing
    /// path; a private workspace beside it is cleaned up after the build.
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
    /// single-threaded. HTML parsing and term generation run in
    /// parallel; database writes are serialised in input order so doc
    /// IDs stay stable regardless of --jobs.
    #[arg(long, default_value_t = 0)]
    jobs: usize,
    /// Store per-doc termlists in the output DB. Default off — matches
    /// modern kiwix ZIMs (libzim sets DB_NO_TERMLIST). Turn this on
    /// when you need to walk terms per document (e.g. for
    /// `xapian-delve -1 -r N`); the resulting DB will be larger and
    /// won't byte-match a kiwix-built reference.
    #[arg(long)]
    keep_termlists: bool,
    /// ICU accent-removal pipeline. `libzim` uses `Lower; NFD; [:M:]
    /// remove; NFC`. `latin` preserves marks outside selected combining
    /// blocks, changing search behavior and diverging from libzim.
    #[arg(long, value_parser = ["libzim", "latin"], default_value = "libzim")]
    accent_rule: String,
    /// Documents buffered in memory between Xapian B-tree flushes
    /// (sets XAPIAN_FLUSH_THRESHOLD). Higher = faster bulk builds,
    /// more memory. Defaults to 50000 (Xapian's own default is 10000);
    /// an already-set XAPIAN_FLUSH_THRESHOLD env var wins over the
    /// default but loses to an explicit flag.
    #[arg(long)]
    flush_threshold: Option<u32>,
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
    /// Optional per-document override for the stemmer language. Empty
    /// or absent means "use the CLI `--language` default". Forwarded
    /// to `Xapian::Stem` (so the literal `english`, `porter`, or any
    /// Snowball language name works), with an ICU fallback for
    /// ISO-639-3 codes (`eng` → `en`).
    #[serde(default)]
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
    // Xapian reads XAPIAN_FLUSH_THRESHOLD when the WritableDatabase is
    // created, so this must happen before Builder::new. We're still
    // single-threaded here (the rayon pool spins up lazily later).
    match args.flush_threshold {
        Some(n) => std::env::set_var("XAPIAN_FLUSH_THRESHOLD", n.to_string()),
        None => {
            if std::env::var_os("XAPIAN_FLUSH_THRESHOLD").is_none() {
                std::env::set_var("XAPIAN_FLUSH_THRESHOLD", DEFAULT_FLUSH_THRESHOLD);
            }
        }
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
    let accent_rule = match args.accent_rule.as_str() {
        "latin" => AccentRule::Latin,
        _ => AccentRule::Libzim,
    };
    let builder = Builder::new(
        &args.output,
        BuilderOptions {
            language: &args.language,
            stopwords: stopwords_text,
            stemmer: &args.stemmer,
            accent_rule,
            keep_termlists: args.keep_termlists,
            mode,
        },
    )?;

    if args.jobs > 0 {
        rayon::ThreadPoolBuilder::new()
            .num_threads(args.jobs)
            .build_global()
            .ok(); // already initialised on a second invocation; harmless.
    }

    // `Send` because the reader crosses into rayon::join; Stdin locks
    // per call, which the 1 MB BufReader amortises away.
    let reader: Box<dyn Read + Send> = if args.input == "-" {
        Box::new(stdin())
    } else {
        Box::new(File::open(&args.input).with_context(|| format!("opening input {}", args.input))?)
    };
    let mut lines = BufReader::with_capacity(1 << 20, reader).lines();

    // Three-stage pipeline, two chunks in flight:
    //   1. read JSONL lines (serial, cheap),
    //   2. parse + tokenise into Xapian documents (rayon workers — the
    //      expensive part: HTML strip, ICU transliteration, stemming),
    //   3. add_document in input order (serial, keeps doc IDs stable).
    // Each loop turn overlaps stage 2 for chunk N with stages 3+1 for
    // chunks N-1 / N+1 via rayon::join, so the writer and the reader
    // never sit idle behind the parsers.
    let mut count: u64 = 0;
    let mut last_report: u64 = 0;
    let mut cur = read_chunk(&mut lines)?;
    let mut pending: Option<Vec<Prepared>> = None;
    while !cur.is_empty() {
        let (io_res, prep_res) = rayon::join(
            || -> Result<Vec<String>> {
                if let Some(docs) = pending.take() {
                    write_docs(&builder, docs, &mut count)?;
                }
                read_chunk(&mut lines)
            },
            || prepare_chunk(&builder, mode, &cur, accent_rule),
        );
        let next = io_res?;
        pending = Some(prep_res?);
        cur = next;
        if !args.quiet && count - last_report >= 1000 {
            eprintln!("{count} docs indexed");
            last_report = count;
        }
    }
    if let Some(docs) = pending.take() {
        write_docs(&builder, docs, &mut count)?;
    }

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

/// Reads up to CHUNK non-empty lines, or CHUNK_BYTES of them, whichever
/// comes first. An empty result means EOF.
///
/// The byte budget matters because a line is one whole ZIM entry: its body
/// is an entire article. Bounding only the line count bounds the number of
/// documents in flight but not their size, and with two chunks pipelined
/// that is 512 unbounded bodies resident. An archive whose entries are
/// large — a scraper that inlined its images as data: URIs, or a crafted
/// one — then drives memory by content size rather than by anything this
/// program chose. A single line larger than the budget is still read whole
/// and processed alone, which is the best that can be done without
/// refusing to index the entry at all.
fn read_chunk(lines: &mut Lines<BufReader<Box<dyn Read + Send>>>) -> Result<Vec<String>> {
    let mut chunk = Vec::with_capacity(CHUNK);
    let mut bytes = 0usize;
    for line in lines {
        let line = line.context("reading input")?;
        if line.trim().is_empty() {
            continue;
        }
        bytes += line.len();
        chunk.push(line);
        if chunk.len() >= CHUNK || bytes >= CHUNK_BYTES {
            break;
        }
    }
    Ok(chunk)
}

/// Parse + tokenise a chunk in parallel. `par_iter().collect()`
/// preserves input order, so the documents come back in the same
/// order the lines were read.
fn prepare_chunk(
    builder: &Builder,
    mode: Mode,
    chunk: &[String],
    rule: AccentRule,
) -> Result<Vec<Prepared>> {
    match mode {
        Mode::Fulltext => chunk
            .par_iter()
            .map(|line| prepare_fulltext(builder, line, rule))
            .collect(),
        Mode::Title => chunk
            .par_iter()
            .map(|line| prepare_title(builder, line))
            .collect(),
    }
}

/// Serial leg: append prepared documents in input order.
fn write_docs(builder: &Builder, docs: Vec<Prepared>, count: &mut u64) -> Result<()> {
    for prepared in docs {
        if let Prepared::Doc(doc) = prepared {
            builder.add_doc(&doc)?;
            *count += 1;
        }
    }
    Ok(())
}

/// A document already parsed + tokenised, ready for the serial
/// add_document leg. Computed in worker threads, applied in order.
enum Prepared {
    Skip,
    Doc(PreparedDoc),
}

fn parse_line(line: &str) -> Result<InputDoc> {
    serde_json::from_str(line).with_context(|| {
        format!(
            "parsing JSONL: {}",
            line.chars().take(80).collect::<String>()
        )
    })
}

/// True when any field that has to cross the C ABI as a NUL-terminated
/// string contains an interior NUL.
///
/// Such an entry cannot be represented as a `CString`, and turning that into
/// a hard error means one malformed entry costs the *whole archive* its
/// index: the builder exits non-zero, writes no database, and zimru — which
/// treats a failed helper as "carry on without indexes" — then ships a ZIM
/// with no search at all. Skipping the single entry is the proportionate
/// response.
fn has_interior_nul(fields: &[&str]) -> bool {
    fields.iter().any(|f| f.as_bytes().contains(&0))
}

fn prepare_title(builder: &Builder, line: &str) -> Result<Prepared> {
    let doc = parse_line(line)?;
    let target_path = doc.target_path.unwrap_or_default();
    let lang = doc.language.unwrap_or_default();
    if has_interior_nul(&[&doc.path, &doc.title, &target_path, &lang]) {
        eprintln!(
            "xapianbuilder: skipping title entry with a NUL byte in path/title: {:?}",
            doc.path.chars().take(60).collect::<String>()
        );
        return Ok(Prepared::Skip);
    }
    let prepared = builder.prepare_title(&doc.path, &doc.title, &target_path, &lang)?;
    Ok(Prepared::Doc(prepared))
}

fn prepare_fulltext(builder: &Builder, line: &str, rule: AccentRule) -> Result<Prepared> {
    let doc = parse_line(line)?;
    let lang = doc.language.unwrap_or_default();
    if has_interior_nul(&[&doc.path, &doc.title, &lang]) {
        eprintln!(
            "xapianbuilder: skipping entry with a NUL byte in path/title: {:?}",
            doc.path.chars().take(60).collect::<String>()
        );
        return Ok(Prepared::Skip);
    }

    if doc.mimetype.starts_with("text/html") {
        let parsed = ParsedDoc::parse_with(doc.body.as_bytes(), rule)
            .with_context(|| format!("parsing HTML for {}", doc.path))?;
        if !parsed.indexing_allowed() {
            return Ok(Prepared::Skip);
        }
        let prepared = builder.prepare_fulltext(FulltextDocument {
            path: &doc.path,
            title: &doc.title,
            content: parsed.content().as_bytes(),
            keywords: parsed.keywords().as_bytes(),
            word_count: parsed.word_count(),
            geo: parsed.geo(),
            language: &lang,
        })?;
        Ok(Prepared::Doc(prepared))
    } else {
        // Non-HTML mimetypes pass through verbatim. libzim only
        // indexes HTML (everything else has no IndexData), so this is
        // an extension: the body lands in the index unmodified — no
        // accent strip, no keyword extraction.
        if doc.body.is_empty() {
            return Ok(Prepared::Skip);
        }
        let word_count = doc.body.split_whitespace().count() as u32;
        let prepared = builder.prepare_fulltext(FulltextDocument {
            path: &doc.path,
            title: &doc.title,
            content: doc.body.as_bytes(),
            keywords: b"",
            word_count,
            geo: None,
            language: &lang,
        })?;
        Ok(Prepared::Doc(prepared))
    }
}
