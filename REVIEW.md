# Review — xapianbuilder

The other three repos in this family each carry a DS4 sweep; xapianbuilder
had none. This is a first pass, scoped to the surface that actually matters
here: **everything this binary consumes is untrusted.** It is handed the
contents of arbitrary ZIM archives — entry paths, titles and whole HTML
bodies — as JSONL on stdin, and it feeds them through a vendored HTML parser
and into Xapian.

Findings are recorded with what was checked, so the ones marked clean are
distinguishable from the ones nobody looked at.

## Fixed

| Finding | What was wrong |
|---|---|
| **[medium] `src/main.rs` — one malformed entry destroys the whole index.** An entry whose path, title or language contains a NUL byte fails `CString::new`, the `?` propagates to `main`, and the process exits non-zero having written **no database at all**. zimru's `IndexHelper` treats a failed helper as "carry on without indexes", so the visible result is a ZIM published with no search — from a single bad entry, with nothing in the archive to indicate why. Reproduced with a three-line JSONL fixture: one bad entry, zero output. | Entries that cannot be represented across the C ABI are now skipped individually with a warning naming the path. Same fixture: bad entry skipped, other two indexed, database written. |
| **[low] `src/main.rs` — chunk budget bounds documents, not bytes.** `CHUNK = 256` caps how many lines are in flight, but a line is a whole ZIM entry including its body. With two chunks pipelined that is 512 unbounded bodies resident, so memory is driven by the archive's content size rather than by anything this program chose — an archive with large entries (inlined data: URIs, or a crafted one) sets the footprint. | Added `CHUNK_BYTES` (64 MiB); a chunk closes on whichever limit is hit first. A single oversized line is still processed alone, which is the most that can be done without refusing to index the entry. |
| **[medium] `tests/golden.rs` — the test helper fabricated results.** `doc_count()` returned `1` when `xapian-delve` was absent, and callers compared that against real expectations. Two consequences, both bad: on a machine without xapian-tools the tests *failed* rather than skipping, contradicting the file's own docstring; and any assertion expecting exactly 1 document would have **passed without checking anything**. | `doc_count()` returns `Option<u32>`; callers skip explicitly and say so. |

## Checked and clean

- **C++/Rust boundary (`cpp/bridge.cc`).** Every `extern "C"` entry point
  wraps its body in `try { … } catch (const std::exception&) { … } catch (...)`,
  so no exception unwinds into Rust. Errors surface through `xb_last_error()`.
  This is the failure mode that matters most here — Xapian throws — and it
  is handled.
- **Ordering under parallelism.** Documents are prepared on rayon workers
  but appended serially in input order, so doc IDs stay stable regardless
  of `--jobs`. That is what makes the output reproducible.
- **Output safety.** The builder refuses to overwrite an existing output
  file (covered by `refuses_to_overwrite`).

## Not yet reviewed

- `cpp/htmlparse.cc`, `cpp/myhtmlparse.cc`, `cpp/namedentities.h` (~700
  lines) — the vendored HTML parser, and the largest untrusted-input
  surface in the repo. It parses attacker-controlled markup in C++. A
  dedicated pass, ideally with a fuzzer over `xb_parse_html`, is the
  highest-value next step and is *not* covered by anything above.
- `src/stopwords.rs`, ICU transliteration paths in `bridge.cc`.

## Validation this repo does have

`bench/search-fidelity.sh` rebuilds an archive with zimru + xapianbuilder
and puts the same queries to the original and the rebuild through upstream
`zimsearch` — a libzim/Xapian binary that knows nothing about how either
index was produced. Queries are sampled from the archive itself, so each
language exercises its own script. On the Chinese archive checked during
the 3.8.0 benchmark run, upstream `zimsearch` returned identical ranked
results from libzim's original index and from xapianbuilder's rebuild.
