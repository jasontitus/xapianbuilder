# Upstream provenance

This file is the public provenance record; building and reviewing the project
does not require the ignored `scratch/` directory. Upstream copyright and
license notices remain in the vendored source files. The repository's
[LICENSE](LICENSE) is GPL-3.0; the vendored parser notices permit GPL-2.0 or later.

## Reference revision and limits

The available local libzim reference checkout identifies its `main` HEAD and
`origin/main` as:

```text
https://github.com/openzim/libzim
dde6c500a7557457ec7117295cfc54442fccb76e
```

[Browse that exact revision](https://github.com/openzim/libzim/tree/dde6c500a7557457ec7117295cfc54442fccb76e).
This is an identified **reference baseline**, not a claim that the original
vendoring event recorded or used that exact commit. The original import did
not include a revision manifest. The local source snapshot and copied writer
reference files were used to recover the comparison below; their original
download dates and the build provenance of old reference ZIMs are unknown.
Native library versions selected by a new build are independent of this
source baseline.

To obtain the baseline without local scratch data:

```sh
git clone https://github.com/openzim/libzim.git libzim-reference
git -C libzim-reference checkout --detach dde6c500a7557457ec7117295cfc54442fccb76e
```

## Vendored parser

| This repository | Upstream path | Local modifications |
| --- | --- | --- |
| `cpp/htmlparse.h` | `src/xapian/htmlparse.h` | Removed libzim/config includes and `LIBZIM_PRIVATE_API`; explicit script/style raw-text state and allocation-free entity lookup. |
| `cpp/htmlparse.cc` | `src/xapian/htmlparse.cc` | Bounds checks for truncated tags/attributes; linear entity decoding with overflow/scalar validation; only matching end tags leave script/style raw text. |
| `cpp/myhtmlparse.h` | `src/xapian/myhtmlparse.h` | Removed libzim include/export annotation; coordinates use `double` instead of `float`. |
| `cpp/myhtmlparse.cc` | `src/xapian/myhtmlparse.cc` | Geo parsing checks complete consumption, classic locale, finite values and coordinate ranges before storing either coordinate. |
| `cpp/namedentities.h` | `src/xapian/namedentities.h` | No local content changes relative to the available baseline. |

The parser's notices identify its earlier BrightStation/Ananova/Olly Betts
origins; preserve those notices rather than attributing all of it solely to
libzim.

SHA-256 of the **upstream reference files**, before these local changes:

```text
1ffd16ec8792a41ad69804687abb1794dcaa723e7703b7d5bf6d6610f21cd262  src/xapian/htmlparse.h
42408dc8b26e6a1aa2392c7c571a5c5e0f1d630e8b5e5fb3342dff1c86feeb6d  src/xapian/htmlparse.cc
b39e1d784dcd56d5dd1375fcf3456895b43b862f1febe32cc653513c98c6f8a3  src/xapian/myhtmlparse.h
daaabeb6bf4e393b96757eaea42f265bfab20d13a7fb779236e3f3aa9c64ae5d  src/xapian/myhtmlparse.cc
049b56722ac264be4b0e8db776b602355a4c18a38b0d7585a826004de6236272  src/xapian/namedentities.h
```

## Stopwords

All 58 files in `data/stopwords/` match the same-named files in the available
reference checkout's `static/stopwords/` byte for byte. No local list edits
were found during the provenance inventory. These are vendored from libzim;
that identifies the immediate source, not independent authorship or a
separately established license for every original list. Refer to
[the upstream tree](https://github.com/openzim/libzim/tree/dde6c500a7557457ec7117295cfc54442fccb76e/static/stopwords)
and its licensing materials when redistributing.

## Writer semantics adapted by the bridge

`cpp/bridge.{h,cc}` is the local C ABI/implementation, not a verbatim vendored
libzim translation unit. Its indexing conventions draw on
[`src/writer`](https://github.com/openzim/libzim/tree/dde6c500a7557457ec7117295cfc54442fccb76e/src/writer),
in particular `xapianIndexer.cpp`, `xapianWorker.cpp`, and
`defaultIndexData.h`. The locally available copied writer reference files
match the corresponding files in the reference checkout; representative
SHA-256 hashes are:

```text
4d3bcc67a74c282881839d22d5d176ab3dc5c2ddff40a9b436ad6d351f49652f  src/writer/xapianIndexer.cpp
ab515da56617634b71e36a27f9c6ba3110caf869d79fafcc9872c32e97aa51ed  src/writer/xapianWorker.cpp
a21e319f619af49122709a201d73f0a7c876d478ddbca9bd73cf79f24f1094d8  src/writer/defaultIndexData.h
```

Local implementation choices include a C ABI consumed by Rust, parallel
preparation with ordered insertion, per-thread ICU transliterators, selectable
accent rules and stemmers, optional termlists, private bulk-build storage,
and single-file compaction. Boundary hardening includes length-bearing
keyword input, RAII ownership, explicit parser/ICU errors, and preserving
Xapian error descriptions. The Rust owner publishes the completed database
without clobbering existing output. None of these local differences implies
that arbitrary output databases are byte-identical to libzim's.

Adversarial reproductions also exposed inherited indexing errors at processing
boundaries. This implementation lowercases with full input context before
batched accent removal and uses NFC-aware joins after mark removal. It does not
reproduce the baseline's Greek-sigma and composition errors at 4096 UTF-16-unit
seams. Script/style literal closing-body tags likewise no longer discard
subsequent visible text. These are deliberate semantic corrections, not claims
of byte-for-byte fidelity to the recorded upstream source.

When updating vendored code, retain notices, record the new exact baseline,
and update this local-modification inventory. For comparison evidence use
the committed tools described in [README.md](README.md#verification), not
private scratch artifacts.
