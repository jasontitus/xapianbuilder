/*
 * xapianbuilder C++/Xapian/ICU implementation.
 * SPDX-License-Identifier: GPL-3.0-or-later
 *
 * This is a local adaptation, not a verbatim libzim translation unit.
 * The following notices apply to the upstream portions adapted here:
 *
 * src/writer/xapianIndexer.cpp (GPL-3.0-or-later):
 * Copyright (C) 2021 Maneesh P M
 * Copyright (C) 2018-2021 Matthieu Gautier
 * Copyright (C) 2011 Emmanuel Engelhart
 *
 * src/writer/xapianWorker.cpp (GPL-2.0-or-later):
 * Copyright (C) 2021 Maneesh P M
 * Copyright (C) 2020-2021 Matthieu Gautier
 *
 * src/writer/defaultIndexData.h (GPL-2.0-or-later):
 * Copyright (C) 2021 Matthieu Gautier
 *
 * src/tools.cpp (GPL-3.0-or-later; name spelling retained from upstream):
 * Copyright (C) 2016-2021 Matthieu Gautier
 * Copyright (C) 2021 Maneeshs P M
 * Copyright (C) 2013-2016 Emmanuel Engelhart
 *
 * src/constants.h (GPL-2.0-or-later):
 * Copyright (C) 2021 Maneesh P M
 *
 * Reference baseline (not a recorded original-import revision):
 * https://github.com/openzim/libzim/tree/dde6c500a7557457ec7117295cfc54442fccb76e
 * See UPSTREAM.md for provenance and differences from that baseline.
 *
 * Local adaptation created 2026-04-27: C ABI, Xapian indexing and ICU
 * normalization outside libzim. Subsequent changes include configurable
 * normalization/stemming and parallel document preparation. Implementation
 * last modified 2026-09-18: boundary/error handling, ownership, and Unicode
 * normalization hardening. Notices added 2026-09-19.
 *
 * This program is free software; you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation; either version 3 of the License, or
 * (at your option) any later version. This does not remove the original
 * GPL-2.0-or-later grants for the upstream portions identified above.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
 * GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License
 * along with this program; if not, see <https://www.gnu.org/licenses/>.
 */

#include "bridge.h"

#include <xapian.h>

#include <unicode/locid.h>
#include <unicode/normalizer2.h>
#include <unicode/translit.h>
#include <unicode/unistr.h>
#include <unicode/utypes.h>
#include <unicode/ucnv.h>

#include "myhtmlparse.h"

#include <atomic>
#include <limits>
#include <cctype>
#include <cmath>
#include <cstring>
#include <map>
#include <memory>
#include <mutex>
#include <sstream>
#include <stdexcept>
#include <string>

// ---- Constants vendored from libzim/src/constants.h --------------------
#define ANCHOR_TERM "0posanchor "
#define MAX_INDEXABLE_TITLE_WORD_SIZE 240

// ---- Per-thread error ring --------------------------------------------
namespace {

thread_local std::string g_last_error;

void set_error(const std::string& msg) { g_last_error = msg; }

// ---- Accent removal + lowercasing --------------------------------------
//
// Apply root-locale lowercase to the whole input before batched accent
// removal. Lowercasing chunks independently loses context at their edges
// (for example Greek final sigma). Accent batches avoid repeated mutation
// of a large UnicodeString; NFC append repairs composition at their joins.

constexpr const char* RULE_LIBZIM =
    "NFD; [:M:] remove; NFC";
// Combining Diacritical Marks blocks: 0300-036F + 1AB0-1AFF (Extended)
// + 1DC0-1DFF (Supplement) + 20D0-20FF (for Symbols). These cover
// Latin/Greek/Cyrillic/IPA accent removal; Thai/Devanagari/Arabic vowel
// signs live elsewhere and pass through.
constexpr const char* RULE_LATIN_ONLY =
    "NFD; "
    "[\\u0300-\\u036F\\u1AB0-\\u1AFF\\u1DC0-\\u1DFF\\u20D0-\\u20FF] remove; "
    "NFC";

icu::Transliterator* build_translit(const std::string& rule_id) {
    UErrorCode status = U_ZERO_ERROR;
    const char* rule = (rule_id == "latin") ? RULE_LATIN_ONLY : RULE_LIBZIM;
    // Compound-ID parser for the normalization/removal pipeline. Lowercase
    // is applied separately with full string context.
    icu::UnicodeString id(rule, "UTF-8");
    std::unique_ptr<icu::Transliterator> t(
        icu::Transliterator::createInstance(id, UTRANS_FORWARD, status));
    if (U_FAILURE(status) || !t) {
        throw std::runtime_error(std::string("ICU transliterator: ") +
                                 u_errorName(status));
    }
    return t.release();
}

std::string remove_accents_lower_with(icu::Transliterator* t, const std::string& s) {
    if (s.empty() || !t) return s;
    if (s.size() > static_cast<size_t>(std::numeric_limits<int32_t>::max())) {
        throw std::length_error("text exceeds ICU's supported length");
    }
    icu::UnicodeString us = icu::UnicodeString::fromUTF8(s);
    us.toLower(icu::Locale::getRoot());
    if (us.isBogus()) throw std::bad_alloc();

    constexpr int32_t BATCH = 4 * 1024;
    if (us.length() <= BATCH) {
        t->transliterate(us);
        if (us.isBogus()) throw std::bad_alloc();
        std::string out;
        us.toUTF8String(out);
        return out;
    }
    UErrorCode status = U_ZERO_ERROR;
    const auto* normalizer = icu::Normalizer2::getNFCInstance(status);
    if (U_FAILURE(status)) {
        throw std::runtime_error(std::string("ICU normalizer: ") + u_errorName(status));
    }

    icu::UnicodeString normalized;
    int32_t pos = 0;
    icu::UnicodeString chunk;
    while (pos < us.length()) {
        int32_t end = us.getChar32Limit(
            us.length() - pos > BATCH ? pos + BATCH : us.length());
        // Each chunk is normalized separately, then joined with NFC boundary
        // repair. Mark deletion can expose composing characters across a seam.
        int32_t len = end - pos;
        chunk.remove();
        us.extract(pos, len, chunk);
        t->transliterate(chunk);
        if (chunk.isBogus()) throw std::bad_alloc();
        normalizer->append(normalized, chunk, status);
        if (U_FAILURE(status) || normalized.isBogus()) {
            throw std::runtime_error(std::string("ICU normalization append: ") + u_errorName(status));
        }
        pos += len;
    }
    std::string out;
    normalized.toUTF8String(out);
    return out;
}

// Per-thread transliterator cache. ICU Transliterator instances are
// not documented as safe for concurrent transliterate() calls (libzim
// shares one static instance across its worker threads, which happens
// to work but is not guaranteed); each thread builds its own copies.
icu::Transliterator* tl_translit(const std::string& rule) {
    thread_local std::map<std::string, std::unique_ptr<icu::Transliterator>>
        cache;
    auto it = cache.find(rule);
    if (it == cache.end()) {
        it = cache.emplace(rule, std::unique_ptr<icu::Transliterator>(
                                     build_translit(rule)))
                 .first;
    }
    return it->second.get();
}

// ASCII-whitespace word counter (mirrors libzim's countWords).
uint32_t count_words(const std::string& s) {
    uint32_t n = 0;
    size_t i = 0, len = s.size();
    while (i < len && std::isspace(static_cast<unsigned char>(s[i]))) i++;
    while (i < len) {
        while (i < len && !std::isspace(static_cast<unsigned char>(s[i]))) i++;
        n++;
        while (i < len && std::isspace(static_cast<unsigned char>(s[i]))) i++;
    }
    return n;
}

// ICU Locale gives ISO-639-1 ("en") from ISO-639-3 ("eng"). Xapian's
// Snowball stemmer takes the ISO-639-1 short code or the English name.
std::string stemmer_lang_for(const std::string& iso6393) {
    icu::Locale loc(iso6393.c_str());
    const char* lang = loc.getLanguage();
    return std::string(lang ? lang : "");
}

// Helpers for term/data accounting in the title special-case path.
size_t get_term_count(const Xapian::Document& d) {
    return std::distance(d.termlist_begin(), d.termlist_end());
}
size_t size_of_indexed_text(const Xapian::Document& d) {
    size_t n = 0;
    for (auto it = d.termlist_begin(); it != d.termlist_end(); ++it) {
        const std::string& term = *it;
        if (term.empty() || term[0] != 'Z')
            n += it.get_wdf() * term.size();
    }
    return n;
}

} // namespace

// ---- IndexBuilder ------------------------------------------------------

// Prepared Xapian document, built entirely in a worker thread and
// handed to xb_add_doc on the writer thread. Xapian's refcounting is
// not atomic, so a doc must only be touched by one thread at a time —
// the prepare/add handoff satisfies that.
struct XbDoc {
    Xapian::Document doc;
};

struct XbBuilder {
    Xapian::WritableDatabase db;
    std::string tmp_path;
    std::string final_path;
    std::string language;          // ISO-639-3 (stored in metadata)
    std::string default_stemmer;   // "" = no stemming
    std::string accent_rule;       // "libzim" | "latin"
    Xapian::SimpleStopper stopper; // populated from stopwords text
    int mode;                      // 0=title, 1=fulltext
    std::atomic<bool> empty{true};
    std::mutex db_mu;              // matches libzim's s_dbaccessLock
};

extern "C" XbBuilder* xb_builder_new(const char* tmp_path,
                                     const char* final_path,
                                     const char* language_iso6393,
                                     const char* stopwords_text,
                                     const char* stemmer_override,
                                     const char* accent_rule,
                                     int keep_termlists,
                                     int mode) {
    try {
        ucnv_setDefaultName("UTF-8");
        auto b = std::unique_ptr<XbBuilder>(new XbBuilder);
        b->tmp_path = tmp_path;
        b->final_path = final_path;
        b->language = language_iso6393 ? language_iso6393 : "";
        const std::string ovr = stemmer_override ? stemmer_override : "";
        if (ovr == "none") {
            b->default_stemmer = "";
        } else if (!ovr.empty()) {
            b->default_stemmer = ovr;
        } else {
            b->default_stemmer = stemmer_lang_for(b->language);
        }
        const std::string rule = accent_rule ? accent_rule : "libzim";
        b->accent_rule = (rule == "latin") ? "latin" : "libzim";
        b->mode = mode;

        // Populate stopper from newline-separated stopwords text.
        std::string sw = stopwords_text ? stopwords_text : "";
        std::istringstream iss(sw);
        std::string word;
        while (std::getline(iss, word, '\n')) {
            if (!word.empty()) b->stopper.add(word);
        }

        // libzim sets DB_NO_TERMLIST so the final compacted DB has no
        // per-doc termlists — current kiwix ZIMs match this. We expose
        // a knob in case a caller needs termlists for tooling. The
        // pre-2024 canonical we initially tested against happened to
        // have termlists due to older Xapian compaction behaviour;
        // do not be misled by that.
        // DB_NO_SYNC + DB_DANGEROUS: the tmp database is scratch space
        // that is deleted on any failure and fully rewritten by
        // compact() on success, so fsync-on-commit and copy-on-write
        // crash safety buy nothing here. Skipping them speeds up
        // commits substantially; the compacted output is byte-for-byte
        // unaffected.
        unsigned int flags = Xapian::DB_CREATE_OR_OVERWRITE |
                             Xapian::DB_NO_SYNC | Xapian::DB_DANGEROUS;
        if (!keep_termlists) flags |= Xapian::DB_NO_TERMLIST;
        b->db = Xapian::WritableDatabase(b->tmp_path, flags);

        if (mode == 0) {
            b->db.set_metadata("valuesmap", "title:0;targetPath:1");
            b->db.set_metadata("kind", "title");
        } else {
            b->db.set_metadata("valuesmap",
                               "title:0;wordcount:1;geo.position:2");
            b->db.set_metadata("kind", "fulltext");
        }
        b->db.set_metadata("data", "fullPath");
        b->db.set_metadata("language", b->language);
        b->db.set_metadata("stopwords", sw);

        return b.release();
    } catch (const Xapian::Error& e) {
        set_error(std::string("xb_builder_new: ") + e.get_description());
        return nullptr;
    } catch (const std::exception& e) {
        set_error(std::string("xb_builder_new: ") + e.what());
        return nullptr;
    } catch (...) {
        set_error("xb_builder_new: unknown error");
        return nullptr;
    }
}

extern "C" void xb_builder_free(XbBuilder* b) { delete b; }

extern "C" int xb_builder_is_empty(const XbBuilder* b) {
    return (b && !b->empty) ? 0 : 1;
}

// Resolve the stemmer string for a per-doc invocation: explicit
// override wins, otherwise fall back to the builder default. The
// cache is thread-local so prepare calls never contend on a lock and
// never share a (non-atomically refcounted) Xapian::Stem across
// threads. Failed lookups are cached as null so unknown languages
// don't pay the exception cost on every document.
static const Xapian::Stem* resolve_stem(const XbBuilder* b,
                                        const char* lang_override) {
    std::string key;
    const std::string ovr = lang_override ? lang_override : "";
    if (ovr == "none") {
        return nullptr;
    } else if (!ovr.empty()) {
        // Treat the override as either ISO-639-3 (translate via ICU)
        // or a literal Snowball language string. We try the literal
        // first (fast path); ICU resolution is only invoked when
        // direct construction fails.
        key = ovr;
    } else {
        key = b->default_stemmer;
    }
    if (key.empty()) return nullptr;

    thread_local std::map<std::string, std::unique_ptr<Xapian::Stem>> cache;
    auto it = cache.find(key);
    if (it != cache.end()) return it->second.get();

    std::unique_ptr<Xapian::Stem> stem;
    // Try literal first.
    try {
        stem.reset(new Xapian::Stem(key));
    } catch (const Xapian::InvalidArgumentError&) {
        // Fall back to ICU language-code resolution ("eng" -> "en").
        std::string resolved = stemmer_lang_for(key);
        if (!resolved.empty() && resolved != key) {
            try {
                stem.reset(new Xapian::Stem(resolved));
            } catch (const Xapian::InvalidArgumentError&) {}
        }
    }
    it = cache.emplace(key, std::move(stem)).first;
    return it->second.get();
}

extern "C" XbDoc* xb_prepare_title(const XbBuilder* b,
                                   const char* path,
                                   const char* title,
                                   const char* target_path,
                                   const char* lang_override) {
    if (!b || b->mode != 0) {
        set_error("xb_prepare_title: builder is not in title mode");
        return nullptr;
    }
    try {
        const std::string path_s = path ? path : "";
        const std::string title_s = title ? title : "";
        const std::string target_s = target_path ? target_path : "";

        Xapian::TermGenerator indexer;
        indexer.set_max_word_length(MAX_INDEXABLE_TITLE_WORD_SIZE);
        indexer.set_flags(Xapian::TermGenerator::FLAG_CJK_NGRAM);
        const Xapian::Stem* s = resolve_stem(b, lang_override);
        if (s) {
            indexer.set_stemmer(*s);
            indexer.set_stemming_strategy(
                Xapian::TermGenerator::STEM_SOME);
        }

        auto out = std::unique_ptr<XbDoc>(new XbDoc);
        Xapian::Document& doc = out->doc;
        std::string full_path = "C/" + path_s;
        doc.set_data(full_path);
        indexer.set_document(doc);

        std::string unaccented = remove_accents_lower_with(
            tl_translit(b->accent_rule), title_s);

        doc.add_value(0, title_s);
        doc.add_value(1, target_s.empty() ? path_s : target_s);

        if (!unaccented.empty()) {
            std::string anchored = ANCHOR_TERM + unaccented;
            indexer.index_text(anchored, 1);
            if (anchored.size() >=
                size_of_indexed_text(doc) + MAX_INDEXABLE_TITLE_WORD_SIZE) {
                set_error("title indexing: too much data lost");
                return nullptr;
            }
            if (get_term_count(doc) == 1) {
                // Only ANCHOR_TERM was added — title is solely
                // non-word characters. Add the whole title as a
                // single term (matches libzim).
                doc.remove_term(*doc.termlist_begin());
                if (unaccented.size() <= MAX_INDEXABLE_TITLE_WORD_SIZE) {
                    doc.add_term(unaccented);
                }
            }
        }
        return out.release();
    } catch (const Xapian::Error& e) {
        set_error(std::string("xb_prepare_title: ") + e.get_description());
        return nullptr;
    } catch (const std::exception& e) {
        set_error(std::string("xb_prepare_title: ") + e.what());
        return nullptr;
    } catch (...) {
        set_error("xb_prepare_title: unknown error");
        return nullptr;
    }
}

extern "C" XbDoc* xb_prepare_fulltext(const XbBuilder* b,
                                      const char* path,
                                      const char* title,
                                      const char* content,
                                      size_t content_len,
                                      const char* keywords,
                                      size_t keywords_len,
                                      uint32_t word_count,
                                      int has_geo,
                                      double latitude,
                                      double longitude,
                                      const char* lang_override) {
    if (!b || b->mode != 1) {
        set_error("xb_prepare_fulltext: builder is not in fulltext mode");
        return nullptr;
    }
    try {
        // HTML parsing omits invalid optional tags. Direct callers must
        // supply finite coordinates within the geographic bounds.
        if (has_geo && (!std::isfinite(latitude) || !std::isfinite(longitude) ||
                        latitude < -90 || latitude > 90 ||
                        longitude < -180 || longitude > 180)) {
            throw Xapian::InvalidArgumentError("invalid geo coordinates");
        }
        const std::string path_s = path ? path : "";
        const std::string title_raw = title ? title : "";
        const std::string content_s =
            content ? std::string(content, content_len) : "";
        const std::string keywords_s =
            keywords ? std::string(keywords, keywords_len) : "";

        // libzim's DefaultIndexData stores removeAccents(title) and
        // feeds the same string to the term generator; mirror that
        // here (value 0 is only used for collapsing, never display).
        const std::string title_s = remove_accents_lower_with(
            tl_translit(b->accent_rule), title_raw);

        Xapian::TermGenerator indexer;
        indexer.set_flags(Xapian::TermGenerator::FLAG_CJK_NGRAM);
        const Xapian::Stem* s = resolve_stem(b, lang_override);
        if (s) {
            indexer.set_stemmer(*s);
            indexer.set_stemming_strategy(
                Xapian::TermGenerator::STEM_ALL);
        }
        indexer.set_stopper(&b->stopper);
        indexer.set_stopper_strategy(Xapian::TermGenerator::STOP_ALL);

        auto out = std::unique_ptr<XbDoc>(new XbDoc);
        Xapian::Document& doc = out->doc;
        indexer.set_document(doc);
        std::string full_path = "C/" + path_s;
        doc.set_data(full_path);
        doc.add_value(0, title_s);
        doc.add_value(1, std::to_string(word_count));
        if (has_geo) {
            std::string geo =
                Xapian::LatLongCoord(latitude, longitude).serialise();
            doc.add_value(2, geo);
        }

        if (!content_s.empty())
            indexer.index_text_without_positions(content_s);

        if (!title_s.empty()) {
            unsigned int boost = (content_s.size() / 500) + 1;
            indexer.index_text_without_positions(title_s, boost);
        }
        if (!keywords_s.empty())
            indexer.index_text_without_positions(keywords_s, 3);

        return out.release();
    } catch (const Xapian::Error& e) {
        set_error(std::string("xb_prepare_fulltext: ") + e.get_description());
        return nullptr;
    } catch (const std::exception& e) {
        set_error(std::string("xb_prepare_fulltext: ") + e.what());
        return nullptr;
    } catch (...) {
        set_error("xb_prepare_fulltext: unknown error");
        return nullptr;
    }
}

extern "C" int xb_add_doc(XbBuilder* b, const XbDoc* d) {
    if (!b || !d) {
        set_error("xb_add_doc: null builder or doc");
        return -1;
    }
    try {
        std::lock_guard<std::mutex> lock(b->db_mu);
        b->db.add_document(d->doc);
        b->empty.store(false, std::memory_order_relaxed);
        return 0;
    } catch (const Xapian::Error& e) {
        set_error(std::string("xb_add_doc: ") + e.get_description());
        return -1;
    } catch (const std::exception& e) {
        set_error(std::string("xb_add_doc: ") + e.what());
        return -1;
    } catch (...) {
        set_error("xb_add_doc: unknown error");
        return -1;
    }
}

extern "C" void xb_doc_free(XbDoc* d) { delete d; }

extern "C" int xb_finalize(XbBuilder* b) {
    if (!b) {
        set_error("xb_finalize: null builder");
        return -1;
    }
    try {
        b->db.commit();
        b->db.compact(b->final_path,
                      Xapian::DBCOMPACT_SINGLE_FILE |
                          Xapian::Compactor::FULL);
        b->db.close();
        return 0;
    } catch (const Xapian::Error& e) {
        set_error(std::string("xb_finalize: ") + e.get_description());
        return -1;
    } catch (const std::exception& e) {
        set_error(std::string("xb_finalize: ") + e.what());
        return -1;
    } catch (...) {
        set_error("xb_finalize: unknown error");
        return -1;
    }
}

// ---- HTML parsing wrapper ---------------------------------------------

struct XbParsedDoc {
    std::string content;   // accent-stripped + lowercased
    std::string keywords;  // accent-stripped + lowercased
    uint32_t word_count;   // computed BEFORE accent strip (matches libzim)
    bool indexing_allowed;
    bool has_geo;
    double latitude;
    double longitude;
};

extern "C" XbParsedDoc* xb_parse_html(const char* html, size_t len, const char* accent_rule) {
    try {
        if (!html && len != 0) {
            throw std::invalid_argument("null HTML buffer with nonzero length");
        }
        zim::MyHtmlParser parser;
        std::string body(html ? html : "", html ? len : 0);
        try {
            parser.parse_html(body, "UTF-8", true);
        } catch (bool) {
            // MyHtmlParser stops at </body> and robots=noindex with a
            // bool exception. Only those deliberate stops retain partial
            // state; genuine parser failures must reach the C ABI handler.
        }
        auto p = std::unique_ptr<XbParsedDoc>(new XbParsedDoc);
        p->word_count = count_words(parser.dump);
        p->indexing_allowed =
            !parser.dump.empty() && parser.indexing_allowed &&
            (parser.dump.find("NOINDEX") == std::string::npos);
        p->has_geo = parser.has_geoPosition;
        p->latitude = parser.latitude;
        p->longitude = parser.longitude;
        const std::string rule =
            (accent_rule && std::strcmp(accent_rule, "latin") == 0)
                ? "latin"
                : "libzim";
        icu::Transliterator* t = tl_translit(rule);
        p->content = remove_accents_lower_with(t, parser.dump);
        p->keywords = remove_accents_lower_with(t, parser.keywords);
        return p.release();
    } catch (const Xapian::Error& e) {
        set_error(std::string("xb_parse_html: ") + e.get_description());
        return nullptr;
    } catch (const std::exception& e) {
        set_error(std::string("xb_parse_html: ") + e.what());
        return nullptr;
    } catch (...) {
        set_error("xb_parse_html: unknown error");
        return nullptr;
    }
}

extern "C" const char* xb_pd_content(const XbParsedDoc* p, size_t* out_len) {
    if (out_len) *out_len = p ? p->content.size() : 0;
    return p ? p->content.c_str() : "";
}
extern "C" const char* xb_pd_keywords(const XbParsedDoc* p, size_t* out_len) {
    if (out_len) *out_len = p ? p->keywords.size() : 0;
    return p ? p->keywords.c_str() : "";
}
extern "C" uint32_t xb_pd_word_count(const XbParsedDoc* p) {
    return p ? p->word_count : 0;
}
extern "C" int xb_pd_indexing_allowed(const XbParsedDoc* p) {
    return p && p->indexing_allowed ? 1 : 0;
}
extern "C" int xb_pd_has_geo(const XbParsedDoc* p) {
    return p && p->has_geo ? 1 : 0;
}
extern "C" double xb_pd_latitude(const XbParsedDoc* p) {
    return p ? p->latitude : 0.0;
}
extern "C" double xb_pd_longitude(const XbParsedDoc* p) {
    return p ? p->longitude : 0.0;
}
extern "C" void xb_pd_free(XbParsedDoc* p) { delete p; }

extern "C" const char* xb_last_error(void) { return g_last_error.c_str(); }
