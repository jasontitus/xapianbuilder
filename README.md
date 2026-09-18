# xapianbuilder

A standalone helper that builds Xapian search indexes for ZIM writers. It
implements the title/fulltext conventions used by libzim; it does not read
or write ZIM archives itself. Callers can invoke it using JSONL on stdin and
embed the resulting database file as a ZIM item.

**Compatibility means database structure and indexing semantics, not identical
file bytes or universally identical search results.** Xapian, ICU, stemmer
versions, input extraction, and index options can affect results. See
[Verification](#verification), [UPSTREAM.md](UPSTREAM.md), and
[REVIEW.md](REVIEW.md) for the scope and limits of the evidence.

## Outputs

One single-file Xapian glass database per invocation:

| Command | ZIM item path | MIME type |
| --- | --- | --- |
| `fulltext` | `X/fulltext/xapian` | `application/octet-stream+xapian` |
| `title` | `X/title/xapian` | `application/octet-stream+xapian` |

The output is ready to embed as an item; it needs no additional database
packaging. The output parent directory must exist and support hard links.
Build work happens in a private temporary directory in that parent. Only a
completed compacted database is published, using a no-clobber hard link:
existing paths, including symlinks, are not replaced. This is not a guarantee
of durability after power loss. A killed process can leave its private
working directory behind.

### Index conventions

These conventions follow the libzim reference identified in
[UPSTREAM.md](UPSTREAM.md), not an unspecified moving “current libzim.”

Both databases use metadata `data=fullPath`, `language`, `stopwords`, `kind`,
and `valuesmap`. Document data is `C/` followed by the input path.

**Title:** `kind=title`, `valuesmap=title:0;targetPath:1`. Value 0 stores the
original title; value 1 stores the redirect target or the entry path.
Titles are normalized and indexed with positions, `FLAG_CJK_NGRAM`, a
240-byte maximum word length, and `STEM_SOME` (originals plus Z-prefixed
stems). The `0posanchor` prefix supports title-start suggestions. Titles
containing only non-word characters use the parser's special title path.

**Fulltext:** `kind=fulltext`, `valuesmap=title:0;wordcount:1;geo.position:2`.
Value 0 is the normalized title, value 1 a decimal word count, and optional
value 2 a serialized `Xapian::LatLongCoord`. Text is indexed without
positions, using `FLAG_CJK_NGRAM`, `STEM_ALL`, and `STOP_ALL`. Content weight
is 1, title weight is `contentLength / 500 + 1`, and keyword weight is 3.
Malformed or out-of-range geo metadata is ignored rather than stored.

## CLI

```text
xapianbuilder <fulltext|title|version> [OPTIONS]

--input <FILE>            JSONL input; "-" means stdin
--output <FILE>           New single-file database; never overwrites
--language <CODE>         Default language and stopword filename
--stemmer <NAME>          Override derived stemmer; "none" disables it
--stopwords-file <PATH>   Explicit stopword list
--resolve-stopwords      Resolve ISO-639-3 to short stopword filenames
--skip-if-empty          Do not publish a database if no docs were indexed
--jobs <N>               Worker threads; 0 chooses automatically
--flush-threshold <N>    Xapian flush threshold; default 50000
--keep-termlists         Retain per-document termlists (off by default)
--accent-rule <RULE>     "libzim" (default) or "latin"
--quiet                  Suppress progress messages
```

Use `xapianbuilder fulltext --help` or `title --help` for the complete CLI.
`--language eng` follows libzim's metadata convention, but vendored
stopword filenames use short codes such as `en`. Use `--resolve-stopwords`
to map `eng` to `en`, or pass an explicit stopword file. Resolution is off
by default. Empty language disables automatic stemming. `--stemmer english`
(or `en`) selects Xapian's English stemmer; `porter` selects its Porter
stemmer. These options do not guarantee reproduction of an older archive.

A document's nonempty `language` overrides the default stemmer selection
for that document; it does not select a separate per-document stopword list.

### JSONL input

One JSON object per line, for example:

```json
{"path":"Mollusca","title":"Mollusca","mimetype":"text/html","body":"<p>Molluscs are animals.</p>","language":"eng","target_path":""}
```

- `path`: archive-relative entry path; becomes `C/` + path in document data.
- `title`: original title; do not pre-normalize it.
- `mimetype`: `text/html` enables the vendored HTML parser, including
  keywords, geo metadata, and NOINDEX handling. Other types use plain text.
- `body`: UTF-8 content; can be empty in title mode.
- `language`: optional per-document stemmer override; empty inherits default.
- `target_path`: optional redirect target, used in title mode.

Input is streamed, but processing chunks and prepared documents consume
memory. The chunk byte budget is not a total memory cap: an individual
large line is still accepted and may exceed it. Invalid JSON aborts the
invocation; entries rejected at the string/FFI boundary are warned about
and skipped. Exit status 0 indicates success; errors are reported on stderr.

```sh
mkdir -p X/fulltext X/title
xapianbuilder fulltext --input corpus.jsonl --output X/fulltext/xapian --language eng
xapianbuilder title --input titles.jsonl --output X/title/xapian --language eng
```

### Accent rules and multilingual limits

The default `libzim` ICU pipeline is `Lower; NFD; [:M:] remove; NFC`. It
removes all Unicode marks, including marks meaningful in non-Latin scripts.
The experimental `latin` rule limits removal to selected combining-mark
blocks (`U+0300..036F`, `U+1AB0..1AFF`, `U+1DC0..1DFF`, `U+20D0..20FF`).
Those blocks are not a script-aware definition of Latin: this option is not
a guarantee of correct segmentation or better search in any language.

Changing the rule can diverge from libzim, and an index-only change may not
match the reader's query normalization. Multilingual search quality needs
end-to-end checks with the actual reader and corpus; no universal script
correctness claim is made here.

Deliberate corrections relative to the recorded libzim source baseline:
script/style contents cannot terminate the enclosing body via a literal
`</body>`, and lowercasing retains whole-string context across processing
batches (for example Greek final sigma). NFC-aware joins preserve composing
sequences such as Hangul Jamo even when mark removal exposes characters
across a batch boundary. These edge cases can produce
different, more complete terms than the historical implementation. Entity
decoding is a linear read/write pass rather than repeated suffix replacement.
None of these fixes makes this legacy extractor a complete HTML5 parser.

## Building

Required: Rust/Cargo (stable), a C++17 compiler, `pkg-config`, Xapian development
headers/libraries (`xapian-core.pc`), and ICU development headers/libraries
(`icu-uc.pc`, `icu-i18n.pc`). Native library versions are **not pinned** by
Cargo.lock. No minimum Rust version or exhaustive native-version support
matrix has been established. CI exercises the packages available on its
named Ubuntu/macOS runners; package updates can change behavior.

macOS (Intel or Apple Silicon, Homebrew):

```sh
brew install rust pkgconf xapian icu4c
cargo build --release --locked
```

Linux (Debian/Ubuntu):

```sh
sudo apt-get update
sudo apt-get install build-essential pkg-config libxapian-dev libicu-dev xapian-tools
# Install stable Rust/Cargo separately if not already available.
cargo build --release --locked
```

`build.rs` uses the Rust `pkg-config` crate. On native macOS builds without
explicit pkg-config path configuration it asks `brew --prefix icu4c` for
Homebrew's keg-only ICU location, with Intel and ARM unversioned-prefix
fallbacks. Explicit `PKG_CONFIG_PATH`, `PKG_CONFIG_LIBDIR`, and
`PKG_CONFIG_SYSROOT_DIR` (including target-qualified forms) take precedence.
Cross builds need their own target pkg-config/compiler configuration; host
Homebrew paths are not injected. The `pkg-config` and `cc` crates also honor
their standard static/dynamic and compiler environment overrides.

The binary is `target/release/xapianbuilder`. The C++ bridge is compiled into
it, but system Xapian, ICU, and C++ runtime libraries are ordinarily linked
dynamically. Neither a self-contained static executable nor a fixed size is
guaranteed. Inspect deployment dependencies with `otool -L` on macOS or
`ldd` on Linux. Static linking needs compatible static libraries and their
transitive dependencies; it is not the default release procedure.

`Cargo.lock` is committed because this repository ships an application.
Use `--locked` in CI and release builds; dependency updates should include
the lockfile and undergo review. This fixes Rust dependency resolution,
not native packages, toolchains, or output bytes.

## Verification

Install `xapian-delve` (`xapian-tools` on Debian/Ubuntu; included in Homebrew's
`xapian` package). Tests require it and fail if it is missing; there is no
success-shaped fallback. Some distributions call the command `delve`;
provide an `xapian-delve` symlink on PATH in that case.

```sh
cargo test --locked
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
xapian-delve X/fulltext/xapian
xapian-delve -1 -d -V -r 1 X/fulltext/xapian
```

### Reproducible libzim comparison

The committed small corpus and `bench/compare-upstream.py` compare this
builder to the installed libzim through its public writer API. They do not
require ignored `scratch/` data, a downloaded Wikipedia archive, or zimru.
Additional prerequisites are Python 3.9 or newer, libzim development files, and
a C++17 compiler. For example:

```sh
# Debian/Ubuntu, in addition to the build prerequisites:
sudo apt-get install libzim-dev python3 git
# macOS, instead:
brew install libzim python
export PKG_CONFIG_PATH="$(brew --prefix icu4c)/lib/pkgconfig:$(brew --prefix xapian)/lib/pkgconfig:$(brew --prefix libzim)/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
```

Run the comparison after building:

```sh
cargo build --release --locked
python3 bench/test-search-fidelity.py
python3 bench/compare-upstream.py --builder target/release/xapianbuilder
python3 bench/test-upstream-values.py --builder target/release/xapianbuilder
```

The comparison retains a unique workspace/report and compares logical
metadata, documents, declared value slots, terms, postings, and positions, ignoring file
UUIDs and document-ID allocation. This is a fixture-specific differential
check, not a proof of equal search ranking on arbitrary archives. Preserve
the report and exact native dependency/toolchain versions when reporting
results. CI runs this same lane on Linux and macOS.

The value-only regression changes a display title while keeping its normalized
terms unchanged. It proves the comparator rejects that difference even when
termlists are disabled; enumerating document values alone is insufficient for
such databases.

CI uses an independent **libzim 9.8.1** writer, built from commit
`351df21b348fdafe6566a472a0d9bf8d965bde48`. The comparator also accepts
system libzim installations, but does not hide version-dependent mismatches.
For example, libzim 8.1.1 differs on this fixture's CJK and symbol-only titles;
its comparison correctly fails. That is a semantic difference, not evidence
that older readers cannot open the database format.

To reproduce the pinned reference instead of a distribution's older package,
install Meson/Ninja plus zstd and lzma development libraries (`meson
ninja-build libzstd-dev liblzma-dev` on Debian/Ubuntu; `meson ninja zstd xz`
with Homebrew), then:

```sh
reference=$(mktemp -d)
export LIBZIM_REFERENCE_REVISION=351df21b348fdafe6566a472a0d9bf8d965bde48
git init "$reference/source"
git -C "$reference/source" fetch --depth=1 https://github.com/openzim/libzim.git "$LIBZIM_REFERENCE_REVISION"
git -C "$reference/source" checkout --detach FETCH_HEAD
meson setup "$reference/build" "$reference/source" \
  --prefix="$reference/install" --libdir=lib --buildtype=release \
  --wrap-mode=nofallback -Dtests=false -Dexamples=false -Dwerror=false
meson compile -C "$reference/build" -j2
meson install -C "$reference/build"
export PKG_CONFIG_PATH="$reference/install/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
export LD_LIBRARY_PATH="$reference/install/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export DYLD_LIBRARY_PATH="$reference/install/lib${DYLD_LIBRARY_PATH:+:$DYLD_LIBRARY_PATH}"
python3 bench/compare-upstream.py --builder target/release/xapianbuilder
```

### Optional archive-level comparison

`bench/search-fidelity.sh` needs external zimru `zimrecreate`/`zimdump`,
upstream `zimsearch`, and source ZIMs. Tool settings name executables, not shell
commands:

```sh
UP_SEARCH=/path/to/upstream/zimsearch \
ZIMRU_RECREATE=/path/to/zimru/zimrecreate \
ZIMRU_DUMP=/path/to/zimru/zimdump \
XAPIANBUILDER="$PWD/target/release/xapianbuilder" \
QUERIES=25 \
bench/search-fidelity.sh /path/to/reference.zim
```

Defaults use tools from PATH. `UPSTREAM_DIR` can supply the directory
containing `zimsearch` when `UP_SEARCH` is unset. `OUT` selects an existing
parent for a new retained workspace; absent `OUT`, the system temporary
directory is used. `PYTHON` selects the Python executable for the wrapper.
Reports, command logs, and rebuilt archives remain in that workspace; input
archives and caller-owned output paths are not overwritten.

Exit statuses: **0** means equal sampled ranked-title lists with at least one
nonempty baseline per archive; **1** means a mismatch; **2** means an input,
dependency, execution, or output-format failure; **3** means an inconclusive
all-empty baseline. The wrapper rejects unrecognized `zimsearch` output,
including diagnostics from tool versions that report errors while exiting 0.
Comparing titles alone cannot distinguish different hits with identical
titles. This lane is not a substitute for the committed fixture comparison,
and sampling cannot establish exhaustive ranking or corpus equivalence.

Previous README figures came from local, ignored reference archives and
ad-hoc measurements. They are not shipped reproducible baselines and are
not release acceptance evidence. No current throughput, speedup, byte
identity, or multilingual-quality guarantee is inferred from them.

## Performance and ordering

Document preparation is parallelized; database additions occur in input
order. Stable insertion order makes doc IDs predictable for the same
accepted inputs, but does not imply deterministic database bytes. Temporary
Xapian databases use bulk-build flags and a configurable flush threshold.
These trade durability/memory for throughput during construction; gains
depend on workload, native libraries, hardware, and job count. Benchmark
on the target system rather than assuming a fixed speedup.

## Integration and licensing

Invoke the helper as a separate process, stream JSONL, close stdin, wait for
successful exit, and only then ingest the output file as the appropriate
ZIM item. Decide explicitly how your application handles helper failures;
missing search indexes must not be silently presented as successful builds.

This project is **GPL-3.0-or-later**; see [LICENSE](LICENSE). It links Xapian
(GPL-2.0-or-later), uses ICU (Unicode/ICU licensing), and includes libzim-derived
code (GPL-2.0-or-later). Retain upstream notices and comply with the licenses
of all components you distribute. [UPSTREAM.md](UPSTREAM.md) records vendored
source provenance and local changes.

The process boundary is an architectural choice, **not a blanket exemption
from GPL obligations for a caller**. The FSF's
[GPL FAQ on aggregation](https://www.gnu.org/licenses/gpl-faq.html#MereAggregation)
says pipes, sockets, and command-line arguments normally connect separate
programs, but also says the semantics and intimacy of communication matter.
The same guidance permits mere aggregation of independent programs under
different licenses. Bundling two executables is therefore not automatically
forbidden, nor does separation alone guarantee legal independence.

Distributors must meet the applicable GPLv3 source-conveyance requirements
(especially [sections 4–6](https://www.gnu.org/licenses/gpl-3.0.html)), preserve
notices, and provide corresponding source through a permitted mechanism.
A written offer is not interchangeable with every distribution option.
Assess the actual integration and distribution; this is not legal advice.
Do not copy or translate GPL-covered implementation into a permissively
licensed caller on the assumption that subprocess use authorizes it.

## Scope

- No ZIM I/O or search API; callers package databases and readers query them.
- No stable ABI across Xapian major versions or guarantee against all libzim
  releases. Native dependencies are discovered from the build environment.
- No proof that the native parser is safe for every hostile input. See
  [REVIEW.md](REVIEW.md) for security-review coverage and remaining risks.
