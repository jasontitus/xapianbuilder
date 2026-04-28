# xapianbuilder

A standalone helper binary that builds **Xapian search indexes** for ZIM
files, byte-compatible with what `libzim`'s `zimwriterfs` produces and
what `kiwix-serve`/`libzim` readers consume.

It is designed to be invoked as a subprocess by [zimru](../zimru) (or
any other ZIM writer) — communicates by file/stdin/stdout only, no
shared library, no FFI into the caller, no shared in-process state.

That separation is deliberate: it lets Xapian (GPL-2-or-later) and any
code borrowed from libzim (GPL-2-or-later) live entirely inside this
binary, while leaving the caller free to be MIT/Apache/whatever. See
[License posture](#license-posture) for the rules.

---

## What it produces

Two single-file Xapian glass databases, one per CLI invocation:

| Command  | ZIM path            | Mimetype                            |
| -------- | ------------------- | ----------------------------------- |
| fulltext | `X/fulltext/xapian` | `application/octet-stream+xapian`   |
| title    | `X/title/xapian`    | `application/octet-stream+xapian`   |

Each output file is the complete database — kiwix uses Xapian's
single-file glass format (`DBCOMPACT_SINGLE_FILE`), so the bytes that
land in the ZIM blob *are* the bytes xapianbuilder writes. There is
**no separate packaging step**; if you want the database as a ZIM
item, just feed the file straight into your writer's `add_item` call.

### Format we reproduce

Verified against the on-disk format produced by current libzim
(`src/writer/xapianIndexer.cpp` + `xapianWorker.cpp`, GPL-2-or-later):

**Both DBs**

- Single-file glass DB with magic `\x0f\rXapian Glass`.
- Metadata: `data="fullPath"`, `language=<ISO-639-3>`,
  `stopwords=<full text>`, plus the `kind`/`valuesmap` pair below.
- Doc data: `"C/" + path`.

**Title DB**

- `kind=title`, `valuesmap=title:0;targetPath:1`.
- Positions ON.
- `value 0` = original title (with accents). `value 1` = `path` for
  regular entries, `target_path` for redirects.
- TermGenerator: `FLAG_CJK_NGRAM`, `set_max_word_length(240)`,
  `STEM_SOME` (Z-prefix stems + originals).
- Indexes `"0posanchor " + lowercase(removeAccents(title))` with
  weight 1 — the `0posanchor` prefix lets kiwix's suggester
  prefix-match against the title start. If the title is solely
  non-word characters the whole title is added as a single term
  instead (libzim's edge-case path).

**Fulltext DB**

- `kind=fulltext`, `valuesmap=title:0;wordcount:1;geo.position:2`.
- Positions OFF.
- `value 0` = title, `value 1` = wordcount as decimal string,
  `value 2` = serialised `Xapian::LatLongCoord` if a `geo.position`
  meta tag was present.
- TermGenerator: `FLAG_CJK_NGRAM`, `STEM_ALL` (no Z prefix —
  stemmed terms replace originals), `STOP_ALL` stopper.
- `index_text_without_positions(content, 1)`,
  `index_text_without_positions(title, contentLength/500 + 1)`,
  `index_text_without_positions(keywords, 3)`.

---

## CLI

```
xapianbuilder <COMMAND> [OPTIONS]

Commands:
  fulltext   Build the X/fulltext/xapian database
  title      Build the X/title/xapian database
  version    Print the binary version

Common options:
  --input <FILE>            JSONL stream (one doc per line). "-" for stdin.
  --output <FILE>           Path for the single-file glass DB. Refuses
                            to overwrite an existing file.
  --language <CODE>         Forwarded to ICU for stemmer selection and
                            used as the stopwords filename. libzim's
                            convention is ISO-639-3 ("eng"); pass the
                            2-letter code ("en") to load the vendored
                            stopword list, since libzim ships those by
                            short code. Empty disables stemming.
  --stemmer <NAME>          Override the auto-derived stemmer string.
                            "english" / "en" = Porter2 (current libzim,
                            default), "porter" = Porter1 (older kiwix
                            ZIMs), "none" = disable, "" = derive from
                            --language.
  --stopwords-file <PATH>   Explicit stopword list (overrides defaults).
  --resolve-stopwords       Translate ISO-639-3 → ISO-639-1 to find a
                            vendored list ("eng" → load `en`). Off by
                            default to match libzim's exact-filename
                            lookup behaviour.
  --skip-if-empty           Don't write the output file if zero docs
                            were indexed (matches libzim).
  --jobs <N>                Worker thread count (0 = num_cpus, 1 =
                            single-threaded). HTML parsing runs in
                            parallel; database writes are serialised
                            in input order so doc IDs stay stable
                            regardless of --jobs.
  --quiet                   Suppress progress output.
```

Exit codes: 0 on success, nonzero on any error (message on stderr).

### Input format

JSON Lines, one document per line:

```json
{
  "path": "Mollusca",
  "title": "Mollusca",
  "mimetype": "text/html",
  "body": "<html>…</html>",
  "language": "eng",
  "target_path": ""
}
```

- `path` — entry path inside the ZIM. Becomes the doc's data field as
  `"C/" + path`.
- `title` — original title with accents preserved. Stored verbatim in
  value slot 0; the term generator gets a lowercased+accent-stripped
  copy.
- `mimetype` — `text/html` runs through libzim's `MyHtmlParser`
  (extracts dump, keywords from `<meta name="keywords">`, and
  `geo.position` if present, plus `indexing_allowed` / NOINDEX gating).
  Anything else: the body is indexed as plain text, no metadata
  extraction.
- `body` — UTF-8 content. May be empty for title-only docs.
- `language` (optional) — currently ignored; the CLI `--language` is
  authoritative. The field is accepted so callers can ship it now.
- `target_path` (title mode only) — set on redirects so the title
  DB stores the redirect target in `value 1`.

Bodies can be large; the input stream is read line-by-line so callers
can pipe straight from a generator without buffering the whole corpus.

### Example

```sh
target/release/xapianbuilder fulltext \
    --input corpus.jsonl \
    --output X/fulltext/xapian \
    --language eng

target/release/xapianbuilder title \
    --input titles.jsonl \
    --output X/title/xapian \
    --language eng
```

---

## Building

System prerequisites (macOS Homebrew names):

```sh
brew install rust xapian icu4c
# zim-tools is optional, used for inspecting reference ZIMs:
brew install zim-tools
```

`build.rs` finds `xapian-core` and `icu4c` via `pkg-config` and adds
Homebrew's keg-only `icu4c@78` pkgconfig path automatically. On Linux,
install the equivalent `libxapian-dev` + `libicu-dev` packages.

Then:

```sh
cargo build --release
```

The release binary is `target/release/xapianbuilder` (~1.4 MB on
arm64-darwin, statically wrapping libxapian/libicu/libstdc++).

---

## Verifying output

The repo's `scratch/` directory (gitignored) holds the canonical
reference DBs extracted from a 2024 Wikipedia ZIM and the libzim source
they were compared against. To inspect any DB xapianbuilder produces:

```sh
xapian-delve <db>                # summary: doc count, positions, lengths
xapian-delve -1 -d -V -r 1 <db>  # term list / data / values for doc 1
xapian-delve -A 0pos <db>        # all terms with a given prefix
```

A 2024-01 reference ZIM diverges from current libzim on three points
(legacy `A/` prefix in doc data; no `0posanchor` term; no value-1 in
title DB). xapianbuilder produces the *modern* format that current
libzim and kiwix.org's published ZIMs use.

---

## Repo layout

```
xapianbuilder/
├── Cargo.toml             # GPL-3.0-or-later
├── build.rs               # cc-rs builds cpp/, links xapian + icu via pkg-config
├── README.md              # this file
├── cpp/
│   ├── bridge.{h,cc}      # C ABI: IndexBuilder + parse_html
│   ├── htmlparse.{h,cc}   # vendored from libzim/src/xapian/, GPL-2+
│   ├── myhtmlparse.{h,cc} # vendored from libzim/src/xapian/, GPL-2+
│   └── namedentities.h    # vendored from libzim/src/xapian/, GPL-2+
├── data/stopwords/{af,ar,…,zh,zu}   # 58 lists vendored from libzim/static/
├── src/
│   ├── main.rs            # CLI dispatch
│   ├── lib.rs             # safe RAII over the C ABI
│   ├── ffi.rs             # raw extern "C" bindings
│   ├── parse.rs           # ParsedDoc wrapper
│   └── stopwords.rs       # ISO-639-3 → vendored stopword text lookup
└── scratch/               # local reference material (gitignored)
```

---

## Integration with zimru

zimru spawns xapianbuilder as a subprocess and ingests the resulting
file as a ZIM blob. Sketch:

```rust
let mut helper = std::process::Command::new("xapianbuilder")
    .args(["fulltext",
           "--input",   "-",
           "--output",  &out_path,
           "--language", "eng"])
    .stdin(std::process::Stdio::piped())
    .spawn()?;

let mut stdin = helper.stdin.take().unwrap();
for entry in archive.iter_content() {
    serde_json::to_writer(&mut stdin, &entry.to_index_doc())?;
    stdin.write_all(b"\n")?;
}
drop(stdin);
assert!(helper.wait()?.success());

let blob = std::fs::read(&out_path)?;
creator.add_item(Item::new(
    "X/fulltext/xapian",
    "",
    "application/octet-stream+xapian",
    blob,
));
```

zimru should:

- Locate `xapianbuilder` via `$PATH`, then a `--xapianbuilder-path`
  flag, then a build-time default. **Never** bundle the binary inside
  zimru's release artifact unless you're shipping a combined
  GPL-licensed distribution; ship them as separate executables.
- Treat absence of the helper as a non-fatal warning. ZIMs without
  search indexes are still valid; print a message that tells the user
  how to install xapianbuilder.
- Stream documents in. xapianbuilder reads JSONL line-by-line; nothing
  here buffers the full corpus.

---

## License posture

- **xapianbuilder is GPL-3-or-later.**
- It links `xapian-core` (GPL-2-or-later), uses ICU (Unicode license),
  and vendors HTML-parsing + tokenization code from `libzim`
  (GPL-2-or-later). The combined work is GPL-3+.
- Callers (zimru, other ZIM writers) invoke this binary via
  `Command::spawn` and read its output file. Under the FSF's standard
  interpretation that IPC boundary does **not** create a combined
  work, so callers stay under their own permissive license. The fork+
  exec pattern is exactly how `gcc` invokes `as` and `ld`, and is used
  in MIT/Apache projects to wrap GPL tools (git-lfs → ImageMagick,
  ffmpeg, etc.) without contaminating them.

The boundary rules for contributors:

- Reference libzim/libkiwix freely. Copy code into `cpp/` if useful —
  this binary is GPL-3+.
- **Do not** transcribe what you learned here into zimru. If something
  belongs in both, it lives here, and zimru calls out.
- Ship this binary with its source (or a written offer) per GPL §3/§6.
  Keep the release artifact, license file, and source tarball visibly
  distinct from zimru's.
- zimru must function (even degraded) without xapianbuilder, to keep
  the "two independent programs" framing intact.

---

## Non-goals

- **Querying.** This binary builds indexes; it does not search them.
  The reader path is whatever opens the ZIM (kiwix-serve, libzim's
  `Searcher`, or a future Rust reader).
- **ZIM I/O.** Building, reading, or modifying ZIM files is zimru's
  job. This binary takes documents in, writes a Xapian DB out. It
  never opens a ZIM.
- **Format independence.** This is a Xapian-specific tool. If we ever
  want a non-Xapian search backend, that's a different binary.
- **API stability across Xapian major versions.** We pin the Xapian
  version we link against. Glass format has been stable through 1.4
  and 2.0.

---

## Verification

Building from a 100-article fixture extracted from kiwix's
`wikipedia_en_100_maxi_2024-01.zim` and diffing against the canonical
Xapian DB inside that ZIM:

| Comparison              | Result      |
| ----------------------- | ----------- |
| Document count          | 100 / 100 ✓ |
| Magic + format          | identical   |
| Avg doc length          | 18038 vs 18046 (0.04% diff) |
| Per-doc term overlap    | **99.87%** (canonical-side) |
| Per-doc unique-term diff | 431 of ~360k tokens |

The remaining diff is **stemmer-version drift**: the 2024-01 sample
was built with a stemmer that aggressively stems some words that
modern Porter2 leaves alone (`internal → intern`, `university → univers`,
`emergency → emerg`) but leaves common words like `they/this/was` /
`-ist` nouns alone. No single Xapian stemmer reproduces that pattern
exactly — looks like a custom or pre-1.4-Snowball variant. xapianbuilder
defaults to Porter2 (matches current libzim); pass `--stemmer porter`
for the closest old-ZIM match.

Modern kiwix.org ZIMs should match xapianbuilder's default output
much more closely; verifying against a 2025-vintage ZIM is the next
on-deck item.

## Known limitations / next steps

- **End-to-end CJK/RTL coverage untested.** `FLAG_CJK_NGRAM` is set
  and ICU handles every script in accent removal, but no fixture run
  yet against Chinese / Arabic / Japanese ZIMs.
- **Per-doc language override unused.** The JSONL `language` field is
  parsed but ignored — CLI `--language` wins. Wire this through if we
  start seeing mixed-language ZIMs (`zh-Hant` + `en` etc.).
- **No verification against a 2025-vintage ZIM yet.** Target: pull a
  fresh `wikipedia_en_*` from kiwix.org/library and re-run the term
  diff. Expecting >99.99% overlap.
