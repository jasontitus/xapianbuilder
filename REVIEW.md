# Pre-publication review

## Scope and evidence status

This review treats JSONL strings, HTML bodies, paths, and optional metadata as
untrusted inputs. It covers the CLI, Rust/native boundary, the vendored HTML
parser's reported defects, output publication, build discovery, and comparison
tooling. It is a bounded review, **not an exhaustive audit or a bug-free claim**.

## Attribution follow-up (2026-09-19)

The publication notice review established the Stopwords ISO origin and MIT
grants for all 58 bundled lists. Every list's Git blob matched its upstream
per-language repository; the 57 Gene Diaz notices and the separate Gujarati
notice were verified and included verbatim. No stopword content changed.

The bridge now carries scoped upstream copyright/license notices. Modified
parser files retain their original notice blocks and identify local changes
and dates; an unmodified GPLv2 license text accompanies the retained
GPL-2.0-or-later grants. Native source changes in this follow-up are comments
only. Rust dependency license metadata was also checked against `Cargo.lock`;
binary packaging still requires the notices and source appropriate to what
is actually distributed. See [UPSTREAM.md](UPSTREAM.md) for evidence and scope.

This is an attribution/compliance-material update, not a clean-room claim,
relicensing of upstream material, or legal certification. Existing Git history
is preserved; the documentation does not rely on a history reset.

## Observed validation

The following local runs passed after remediation, on macOS arm64 and a
Debian 12 arm64 container. Hosted GitHub Actions results are separate; adding
a workflow is not evidence that it has run.

| Check | Observed result |
| --- | --- |
| `cargo fmt --all -- --check` | Passed on both platforms. |
| `cargo clippy --locked --all-targets -- -D warnings` | Passed on both platforms. |
| `cargo test --locked --all-targets` | 30 passed on each platform: 14 CLI, 11 native-boundary, 5 output-safety regressions. |
| `cargo build --release --locked` | Passed on both platforms. |
| `RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps` | Passed on macOS. |
| `python3 bench/test-search-fidelity.py` | Seven failure-path tests passed on each platform. |
| `bench/test-upstream-values.py` with the real release builder | A value-only title mutation was rejected on each platform; fulltext stayed equal. |
| Standalone `tests/native_safety.cc` | 128 repeated constructor-failure/parse cycles passed with ASan/UBSan on macOS and Linux, plus LeakSanitizer on Linux. System libraries were not instrumented. |
| Broken inspection-tool scenario | The Thai regression failed as required when an installed `xapian-delve` deliberately returned an error. |
| Upstream fixture comparison | Both platforms matched title (8 documents, 32 terms, 16 values) and fulltext (5 documents, 44 terms, 11 values), including postings, positions and metadata. |

The macOS comparison used installed libzim 9.8.1, Xapian 2.0.0 and ICU 78.3.
Linux used libzim 9.8.1 built from
`351df21b348fdafe6566a472a0d9bf8d965bde48`, Xapian 1.4.22 and ICU 72.1.
Fixture SHA-256:
`b6681c03d33edddf0e16d6ddd288117a0f40bd8569b0416081d123edb8508c4d`.
An additional system-libzim 8.1.1 comparison correctly failed on CJK and
symbol-title differences; those differences were not filtered out to pass CI.
The CI reference is now explicitly pinned to the independently built 9.8.1
writer rather than depending on a distribution's default libzim version.

Independent read-only adversarial passes covered the Rust lifecycle/API,
native parser/boundary, comparison tooling, and documentation/build setup.
Their actionable findings were reproduced and corrected, including a second
normalization seam exposed by mark deletion. A bounded entity-heavy parsing
probe changed from roughly quadratic scaling (161/590/2252 ms for
50k/100k/200k entities) to approximately linear scaling (48/96/194 ms) on the
same local debug build path. This is a regression probe, not a throughput
guarantee or an exhaustive fuzzing campaign.

Publication-source and package-content scans found no credentials or personal
home paths. The public repository-owner URL and required upstream attribution
remain intentional. Git author/committer identities were not anonymized;
history is not claimed to be PII-free.

## Findings addressed in this change set

| Area | Finding and implemented response | Evidence to run |
| --- | --- | --- |
| Output ownership | Predictable temporary paths and direct final-path compaction could collide with existing files or concurrent writers. Rust now owns a private `TempDir` in the output parent, native work stays inside it, and a completed staged file is published by atomic no-clobber hard link. Existing files/symlinks are not replaced. | Output-safety regressions, concurrent destination creation and failure/empty-output cleanup scenarios. |
| Native ownership/errors | Early-return/exception paths could lose resources or diagnostics. Native builder/parser ownership uses RAII; Xapian errors retain their descriptions; unexpected parser failures and ICU initialization failures become Rust errors rather than silent fallback behavior. Expected parser stop signals and unsupported stemmers remain distinct. | `cargo test --locked --test native_regressions`, plus CLI error cases. |
| String boundary | NUL-containing fields must not silently truncate at C-string boundaries. Keywords now carry an explicit length; unsupported entry strings are rejected with an entry warning rather than discarding otherwise valid input. | Keyword/NUL regressions and mixed valid/rejected CLI input. |
| Geo metadata | Unchecked numeric extraction could store invalid/uninitialized coordinates. Parsing now requires full numeric consumption, classic locale, finite values, and valid latitude/longitude ranges; both coordinates are committed together. | Native valid/invalid geo cases and fulltext value inspection. |
| HTML parsing | Truncated tags/attributes could dereference an end iterator; numeric entities needed overflow and Unicode scalar checks. The affected parser paths now check bounds and reject invalid numeric references. | Native malformed HTML regressions and sanitizer job. |
| Adversarial raw text and entities | A literal `</body>` in script/style silently truncated later visible text. Raw-text state now permits only its matching end tag. Repeated entity replacement also caused quadratic suffix copying; decoding now uses a linear in-place read/write pass. | Exact script/style regressions, dense entity cases, bounded before/after timing probe, native sanitizer harness. |
| Normalization boundaries | Independent lowercase batches produced the wrong Greek sigma at a 4096-unit seam. Lowercasing now retains whole-string context, and NFC-aware joins repair composition across accent-removal batches, including deletion-created seams. | Sigma/Hangul/astral boundary regressions for both rules and native sanitizer harness. These intentionally correct inherited upstream edge cases. |
| Input memory | A document-count chunk bound alone allowed many large bodies in flight. A byte budget closes chunks early. This remains a chunk budget, not a hard memory limit; one large line and derived native objects can exceed it. | Chunk-boundary/ordering coverage and workload-specific memory measurement. |
| Golden assertions | Missing inspection tools previously produced invented results or silent skips. Tests now require a real `xapian-delve`; CI installs it and explicitly checks availability. Stemmer and ordering checks assert observed behavior rather than accepting self-comparisons. | `cargo test --locked` with installed inspection tools; missing-tool failure is intentional. |
| Search benchmark | The earlier script could treat tool failures or empty parsed output as identical results and did not reliably propagate failure. The benchmark lane now distinguishes execution/format failures, inconclusive results, and observed mismatches; regression scenarios exercise those outcomes. | `python3 bench/test-search-fidelity.py` and the documented archive lane with actual tools. |
| Value comparison | Runtime inspection showed that Xapian's document-value iterator can be empty without termlists while direct slot reads still return values. The independent snapshotter now queries every `valuesmap` slot explicitly and reports value coverage. | `bench/test-upstream-values.py` builds a real candidate with only a display-title value changed and requires a title-only mismatch; normal comparisons report 16 title values and 11 fulltext values. |
| Reproducibility | Old compatibility/performance claims depended on ignored scratch archives, unspecified versions, or unsupported conclusions. The committed fixture/public-libzim comparator provides a reproducible logical comparison; old numeric/byte-identity claims have been removed. | `python3 bench/compare-upstream.py --builder target/release/xapianbuilder`, retaining its report and versions. |
| Build portability | Shell-split pkg-config output discarded linker information and used a hard-coded ARM Homebrew ICU version. The build uses the `pkg-config` crate and target-aware `cc`, discovers native Homebrew ICU without a version literal, and defers to explicit path configuration. | Locked Linux/macOS builds in CI; custom environment/cross-build checks when those modes are used. |
| Public claims/provenance | Static linkage, version pinning, universal script correctness, and automatic GPL independence were overstated. README qualifies the claims and links FSF guidance; UPSTREAM.md records the known exact reference revision, hashes, modifications, and original-import uncertainty. | Source review, dependency inspection (`otool -L`/`ldd`), reproducible comparison reports. |

## Validation entry points

See [README.md](README.md#verification) for package installation and commands.
The committed CI definition includes:

- Linux/macOS locked tests and release builds, rustfmt, and clippy;
- mandatory Xapian inspection tooling;
- benchmark failure-path regressions and the small public-libzim comparison;
- a Linux AddressSanitizer/UndefinedBehaviorSanitizer build of the local C++
  bridge/parser exercised through the standalone `tests/native_safety.cc`
  C ABI harness, including leak detection and repeated constructor failures.

The sanitizer job instruments this repository's native translation units, not
prebuilt system Xapian/ICU libraries or Rust with a Rust sanitizer toolchain.
It is regression coverage, not a fuzzing campaign. A green differential run
only covers its recorded fixture, settings, and native versions; it does not
establish equality for every corpus, script, search query, or upstream release.

## Remaining limits and risks

- The vendored parser has not undergone exhaustive fuzzing or a complete
  memory-safety proof. Regression cases cover reported defects, not all HTML.
- Insertion order is stable for accepted documents, but database bytes may
  differ (including Xapian UUIDs). Reader normalization/ranking and native
  version drift still need workload-specific end-to-end verification.
- ICU mark removal can affect non-Latin text. The optional block-limited rule
  is not script-aware and is not a universal search-quality improvement.
- A single oversized input line can exceed chunk budgets. There is no total
  input-size, CPU-time, or process-memory limit for hostile workloads.
- Atomic publication depends on same-filesystem hard-link support. It does
  not promise crash durability, and abrupt termination may leave private
  working directories to clean up.
- The libzim reference revision is known, but the exact original import
  revision and old reference archive build environments were not recorded.
- Cross compilation and fully static distributions need explicit dependency
  setup and are not established by the native Linux/macOS CI matrix.

The former “checked and clean” wording overstated what source inspection and
single-sample checks could establish; no such blanket assertion is retained.

## Indexing reference

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

## Build discovery details

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
