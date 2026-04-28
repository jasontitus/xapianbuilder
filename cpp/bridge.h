// xapianbuilder C ABI exposed to Rust.
//
// The whole GPL-derived pipeline (HTML parsing borrowed from omega via
// libzim, accent removal via ICU, term emission via xapian-core) lives
// behind this boundary.

#pragma once

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct XbBuilder XbBuilder;
typedef struct XbParsedDoc XbParsedDoc;

// Mode: 0 = title DB, 1 = fulltext DB.
//
// `stemmer_override` is forwarded straight to `Xapian::Stem` if
// non-empty, bypassing the ICU-derived language code. Pass "porter"
// for old-style stemming (matches pre-2024 kiwix ZIMs); "" to use
// the ICU mapping; "none" or any unknown value leaves stemming off.
//
// `accent_rule` selects the ICU transliterator pipeline applied to
// titles/content/keywords before tokenisation:
//   - "libzim" (default): "Lower; NFD; [:M:] remove; NFC" — strips
//     ALL combining marks. Matches libzim/kiwix. Note this fragments
//     Thai, Devanagari, and Arabic vowel-mark sequences.
//   - "latin":  "Lower; NFD; [<latin-diacritic-blocks>] remove; NFC"
//     — only strips combining marks used by Latin/IPA. Preserves
//     Indic/Thai/Arabic vowel signs at the cost of byte-divergence
//     from libzim on those scripts. Use this for non-Latin corpora.
//
// `keep_termlists` controls whether the WritableDatabase stores per-doc
// termlists. libzim sets DB_NO_TERMLIST (matches modern kiwix ZIMs);
// set this to 1 only when you need termlists for tooling that walks
// documents term-by-term (e.g. xapian-delve -1 -r N).
XbBuilder* xb_builder_new(const char* tmp_path,
                          const char* final_path,
                          const char* language_iso6393,
                          const char* stopwords_text,
                          const char* stemmer_override,
                          const char* accent_rule,
                          int keep_termlists,
                          int mode);
void xb_builder_free(XbBuilder*);

// Returns 1 if no docs have been added yet (mirrors libzim's `empty`
// flag — used by callers to suppress emitting an empty index file).
int xb_builder_is_empty(const XbBuilder*);

// All add_* and finalize return 0 on success, nonzero on error
// (call xb_last_error() for a message).

// `lang_override` (may be NULL or "") forwards to `Xapian::Stem`
// for this single document, overriding the builder-level language.
// Useful for multilingual ZIMs where per-entry language is known.
int xb_add_title(XbBuilder*,
                 const char* path,
                 const char* title,
                 const char* target_path /* "" if not redirect */,
                 const char* lang_override);

// content/keywords are expected pre-processed (lowercased + accents
// stripped) by the caller (parse_html does this for HTML inputs).
int xb_add_fulltext(XbBuilder*,
                    const char* path,
                    const char* title,
                    const char* content,
                    size_t content_len,
                    const char* keywords,
                    uint32_t word_count,
                    int has_geo,
                    double latitude,
                    double longitude,
                    const char* lang_override);

int xb_finalize(XbBuilder*);

// Run libzim's MyHtmlParser on a UTF-8 HTML buffer. Returns NULL if
// the HTML couldn't be parsed at all. content/keywords are returned
// already lowercased + accent-stripped using `accent_rule`
// (`"libzim"` or `"latin"`; see `xb_builder_new`). `accent_rule` may
// be NULL or "" for libzim default.
XbParsedDoc* xb_parse_html(const char* html, size_t len, const char* accent_rule);
const char* xb_pd_content(const XbParsedDoc*, size_t* out_len);
const char* xb_pd_keywords(const XbParsedDoc*);
uint32_t xb_pd_word_count(const XbParsedDoc*);
int xb_pd_indexing_allowed(const XbParsedDoc*);
int xb_pd_has_geo(const XbParsedDoc*);
double xb_pd_latitude(const XbParsedDoc*);
double xb_pd_longitude(const XbParsedDoc*);
void xb_pd_free(XbParsedDoc*);

// Last error from this thread.
const char* xb_last_error(void);

#ifdef __cplusplus
}
#endif
