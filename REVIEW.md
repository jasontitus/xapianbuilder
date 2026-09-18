# Pre-publication review

## Scope and evidence status

This review treats JSONL strings, HTML bodies, paths, and optional metadata as
untrusted inputs. It covers the CLI, Rust/native boundary, the vendored HTML
parser's reported defects, output publication, build discovery, and comparison
tooling. It is a bounded review, **not an exhaustive audit or a bug-free claim**.

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
