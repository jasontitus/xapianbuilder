# xapianbuilder

Build title and fulltext Xapian indexes for ZIM archives, from Rust or a
standalone CLI. This project creates the search databases; your application
packages them into a ZIM. It does not provide archive I/O or a search API.

**Licensing:** this is a GPL-3.0-or-later library, not a permissively licensed
SDK. Review [integration and licensing](#integration-and-licensing) before
embedding it in an application.

## Build requirements

Install stable Rust/Cargo, a C++17 compiler, `pkg-config`, and Xapian/ICU
headers and libraries. **libzim is only needed for upstream comparison tests.**

```sh
# macOS (Homebrew)
brew install rust pkgconf xapian icu4c

# Debian/Ubuntu (install stable Rust separately)
sudo apt-get install build-essential pkg-config libxapian-dev libicu-dev xapian-tools
```

From a checkout, build the CLI with `cargo build --release --locked`.
The executable is `target/release/xapianbuilder`. Xapian, ICU, and the C++
runtime are normally linked dynamically and must also be available at runtime.
`Cargo.lock` does not pin these native dependencies.

If Homebrew finds the wrong installation, select its package metadata explicitly:

```sh
export PKG_CONFIG_PATH="$(brew --prefix icu4c)/lib/pkgconfig:$(brew --prefix xapian)/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
```

## Rust library

Add the Git dependency to your application:

```sh
cargo add xapianbuilder --git https://github.com/jasontitus/xapianbuilder
```

This example creates a fulltext index from HTML:

```rust
use std::path::Path;
use xapianbuilder::{
    parse::ParsedDoc, AccentRule, Builder, BuilderOptions, FulltextDocument, Mode,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let builder = Builder::new(Path::new("fulltext.xapian"), BuilderOptions {
        language: "eng",
        stopwords: "", // Newline-separated words; empty disables filtering.
        stemmer: "",   // Derive from language; "none" disables stemming.
        accent_rule: AccentRule::Libzim,
        keep_termlists: false,
        mode: Mode::Fulltext,
    })?;

    let parsed = ParsedDoc::parse(b"<p>Molluscs are animals.</p>")?;
    if parsed.indexing_allowed() {
        builder.add_fulltext(FulltextDocument {
            path: "Mollusca",
            title: "Mollusca",
            content: parsed.content().as_bytes(),
            keywords: parsed.keywords().as_bytes(),
            word_count: parsed.word_count(),
            geo: parsed.geo(),
            language: "", // Inherit the builder's language.
        })?;
    }
    builder.finalize()?;
    Ok(())
}
```

For a title index, use `Mode::Title` and
`builder.add_title("Mollusca", "Mollusca", "", "")?`. The last two arguments
are an optional redirect target and language override; empty strings use the
entry path and builder language.

Pass original titles, but **normalize fulltext content and keywords first**:
`ParsedDoc::parse` extracts and normalizes HTML using the default rule. For
`AccentRule::Latin`, use `ParsedDoc::parse_with` with the same rule as the builder.
The Rust API takes stopwords explicitly; it does not load a language list for you.

For parallel preparation, use `prepare_title` / `prepare_fulltext`, then
`add_doc` in your desired insertion order. Call `finalize` to publish the file;
dropping an unfinished builder discards its private workspace.
Generate the full API reference with `cargo doc --no-deps --open`.

## CLI

Input is one JSON object per line. Only `path` and `title` are required:

```json
{"path":"Mollusca","title":"Mollusca","mimetype":"text/html","body":"<p>Molluscs are animals.</p>"}
```

- `path` is archive-relative; document data stores it with a `C/` prefix.
- `mimetype: "text/html"` enables HTML extraction, keywords, geo metadata, and
  NOINDEX handling. Other types treat `body` as plain text.
- `body` can be omitted for title indexes.
- Optional `language` overrides the document's stemmer language, not its stopwords.
- Optional `target_path` supplies a redirect target for title indexes.

```sh
mkdir -p X/fulltext X/title
./target/release/xapianbuilder fulltext --input corpus.jsonl --output X/fulltext/xapian --language eng
./target/release/xapianbuilder title --input corpus.jsonl --output X/title/xapian --language eng
```

Use `--input -` for stdin. Useful options:

| Option | Purpose |
| --- | --- |
| `--resolve-stopwords` | Map `eng` to the bundled `en` stopword list; off by default. |
| `--stopwords-file FILE` | Supply one stopword per line explicitly. |
| `--stemmer NAME` | Override stemming, e.g. `english`, `porter`, or `none`. |
| `--jobs N` | Set worker threads; insertion preserves input order. |
| `--skip-if-empty` | Omit the output when no documents were indexed. |

Run `fulltext --help` or `title --help` for all options. Invalid JSON aborts the
build; entries with unsupported NUL-containing path/title fields are warned
about and skipped. Check stderr and the exit status before using the output.

## Output and compatibility

Each invocation produces one single-file Xapian glass database. Embed it at
`X/fulltext/xapian` or `X/title/xapian`, with MIME type
`application/octet-stream+xapian`.

- **No overwrite:** the destination must not exist, and its parent must exist
  and support hard links. Only a completed database is published. This does
  not guarantee power-loss durability; abrupt termination can leave scratch files.
- **Version-sensitive:** fixture comparisons matched libzim 9.8.1 with
  Xapian 2.0.0 / ICU 78.3 on macOS and Xapian 1.4.22 / ICU 72.1 on Linux.
  libzim 8.1.1 produced documented semantic differences. Matching database
  structure does not guarantee identical bytes or search ranking.
- **Multilingual behavior:** the default normalization removes Unicode marks,
  including some meaningful outside Latin scripts. The experimental `latin`
  rule changes behavior and is not script-aware. Test with your actual reader
  and corpus before changing normalization or stemming.
- **Untrusted input:** processing is chunked, but one large document can exceed
  the chunk budget. There is no hard memory/CPU limit or exhaustive parser-safety
  guarantee; apply workload limits outside the library where needed.

## Verification

With `xapian-delve` installed (`xapian-tools` on Debian/Ubuntu, included with
Homebrew Xapian), run `cargo test --locked`.

[REVIEW.md](REVIEW.md) records observed results, limitations, index conventions,
and [reproducible libzim comparisons](REVIEW.md#reproducible-libzim-comparison).
[UPSTREAM.md](UPSTREAM.md) records source provenance and deliberate deviations.

## Integration and licensing

The project is **GPL-3.0-or-later**; see [LICENSE](LICENSE). Xapian and the
vendored parser are GPL-2.0-or-later; the libzim-derived bridge includes
GPL-3.0-or-later code. ICU has its own Unicode/ICU licensing.
Bundled stopword lists are from **Stopwords ISO (MIT)** via libzim; their
copyright notices and license texts are linked in [UPSTREAM.md](UPSTREAM.md#stopwords).

Distributing an application linked to this library raises GPL combined-work
obligations; dynamic linking is not a blanket exemption. Invoking the CLI as
a separate program is an alternative, but independence depends on the actual
integration, not just a process boundary. See the
[FSF's guidance](https://www.gnu.org/licenses/gpl-faq.html#MereAggregation).
Distributing the helper still requires applicable notices and corresponding
source. Generated indexes are not GPL merely because this tool produced them;
rights in their content still apply. This is not legal advice.
