# Upstream provenance

This file is the public provenance record; building and reviewing the project
does not require the ignored `scratch/` directory. Upstream copyright and
license notices remain in the vendored source files. The project is distributed
under **GPL-3.0-or-later**; [LICENSE](LICENSE) contains GPLv3. Components retain
their original grants and notices: the vendored parser is GPL-2.0-or-later,
while the bridge adapts both GPL-2.0-or-later and GPL-3.0-or-later sources.
A copy of [GPLv2](LICENSES/GPL-2.0.txt) is included for the retained version-2-or-later grants.

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

The 58 files in `data/stopwords/` are unchanged copies of the corresponding
libzim `static/stopwords/<code>` files at the reference revision. libzim obtained
these lists from **Stopwords ISO**, not from its own GPL-licensed implementation:
[PR #299](https://github.com/openzim/libzim/pull/299) replaced the earlier lists,
and [PR #447](https://github.com/openzim/libzim/pull/447) refreshed them and added
Gujarati. The corresponding libzim commits are
`f5c4a2d43603adc7364fb1d91a98c46bed767ac9` and
`15f56503f572f0c2e14a4fde5bbea16715f84285`.

On **2026-09-19**, all 58 local list Git blob hashes were verified against
`stopwords-<code>.txt` in the corresponding
[`stopwords-iso/stopwords-<code>`](https://github.com/stopwords-iso) repositories.
All matched exactly. Their per-repository license blobs form these two groups:

| Files | Original copyright and license | Verbatim notice supplied here | Upstream license Git blob |
| --- | --- | --- | --- |
| All 57 lists except `gu` | Copyright (c) 2016 Gene Diaz; MIT | [Stopwords-ISO-MIT.txt](LICENSES/Stopwords-ISO-MIT.txt) | `866a3b472efcbcceec95baf84dc2447b92d2ca82` |
| `gu` | Copyright (c) 2020 Stopwords ISO; MIT | [Stopwords-ISO-gu-MIT.txt](LICENSES/Stopwords-ISO-gu-MIT.txt) | `47b0866c94a1da9aec730da774b1d6b9e4b86cdc` |

Primary license texts: [English collection](https://github.com/stopwords-iso/stopwords-en/blob/master/LICENSE)
and [Gujarati collection](https://github.com/stopwords-iso/stopwords-gu/blob/master/LICENSE).
The MIT copyright, permission and warranty notices must accompany redistributed
copies, including binaries embedding the lists. These grants remain MIT; the
project's GPL license does not replace them. Preserve the notices when exporting
older snapshots containing these same lists as well.

Stopwords ISO credits its compilation sources in
[`CREDITS.md`](https://github.com/stopwords-iso/stopwords-iso/blob/master/CREDITS.md).
This inventory verifies the published MIT grants for the exact collections
used here; it is not a separate legal audit of every earlier source listed in
those credits. No list content was changed during this attribution update.

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

The relevant source notices are file-specific; libzim's root `COPYING` alone
does not describe all of them:

| Referenced source | Original license | Copyright holders named in that source |
| --- | --- | --- |
| `src/writer/xapianIndexer.cpp` | GPL-3.0-or-later | Maneesh P M (2021), Matthieu Gautier (2018–2021), Emmanuel Engelhart (2011). |
| `src/writer/xapianWorker.cpp` | GPL-2.0-or-later | Maneesh P M (2021), Matthieu Gautier (2020–2021). |
| `src/writer/defaultIndexData.h` | GPL-2.0-or-later | Matthieu Gautier (2021). |
| [`src/tools.cpp`](https://github.com/openzim/libzim/blob/dde6c500a7557457ec7117295cfc54442fccb76e/src/tools.cpp) | GPL-3.0-or-later | Matthieu Gautier (2016–2021), Maneeshs P M (2021; spelling as upstream), Emmanuel Engelhart (2013–2016). |
| [`src/constants.h`](https://github.com/openzim/libzim/blob/dde6c500a7557457ec7117295cfc54442fccb76e/src/constants.h) | GPL-2.0-or-later | Maneesh P M (2021). |

`cpp/bridge.cc` retains scoped notices for these adapted portions and grants
GPL-3.0-or-later for the combined local implementation. This does not erase
the original grants for the GPL-2.0-or-later portions. The bridge header is a
local interface, not a verbatim copy of a libzim header.

The local bridge and parser import entered this repository on **2026-04-27**.
The native hardening described below was applied on **2026-09-18**. Prominent
source-file attribution/modification notices were added on **2026-09-19**;
the original parser notices were preserved. `cpp/namedentities.h` remains
unmodified relative to the reference and carries no local modification claim.

Local implementation choices include a C ABI consumed by Rust, parallel
preparation with ordered insertion, per-thread ICU transliterators, selectable
accent rules and stemmers, optional termlists, private bulk-build storage,
and single-file compaction. Boundary hardening includes length-bearing
parsed keyword output, RAII ownership, explicit parser/ICU errors, and preserving
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

## External dependencies and redistribution

Xapian and ICU are external native dependencies, not vendored library source.
[Xapian](https://xapian.org/) is GPL-2.0-or-later. ICU releases carry Unicode/ICU
licenses and additional third-party notices; retain the complete license bundle
from the particular ICU release when redistributing it, not just its top-level
license name. libzim is needed by the comparison tools, not linked by the Rust
library or indexing CLI.

The 51 dependency packages in the current `Cargo.lock` all declare an MIT
licensing option; `unicode-ident` additionally requires Unicode-3.0. This is a
record of package metadata checked on 2026-09-19, not a replacement for those
packages' copyright and license notices. Runtime, build-time and target-specific
packages are included in that count. Recheck the actual packages and their
notices when changing the lockfile or distributing binaries.

For source distributions, preserve this provenance record, the project license,
the bundled component license texts, upstream notices, and dated modification
notices. For binary distributions, also include the notices required by the
code and libraries actually shipped and provide the applicable Corresponding
Source, including build material, through a mechanism permitted by GPLv3
sections 4–6. A link to a moving branch or an arbitrary written offer is not a
universal substitute. Dynamic linking or a subprocess boundary does not by
itself establish an exemption for an integration.

The recorded source baseline is evidence of origin, not an assertion that the
implementation is clean-room or that historical upstream code has been
relicensed. A new Git root, repository rename, or visibility change does not
alter these obligations.
