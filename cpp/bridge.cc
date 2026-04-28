// xapianbuilder C++/Xapian/ICU implementation.
//
// Mirrors libzim's xapianIndexer.cpp + xapianWorker.cpp + tools.cpp
// pipeline so the resulting glass DBs are byte-compatible with kiwix
// readers. See README and scratch/libzim-ref/* for provenance.

#include "bridge.h"

#include <xapian.h>

#include <unicode/locid.h>
#include <unicode/translit.h>
#include <unicode/unistr.h>
#include <unicode/utypes.h>
#include <unicode/ucnv.h>

#include "myhtmlparse.h"

#include <cctype>
#include <cstring>
#include <map>
#include <memory>
#include <mutex>
#include <sstream>
#include <string>

// ---- Constants vendored from libzim/src/constants.h --------------------
#define ANCHOR_TERM "0posanchor "
#define MAX_INDEXABLE_TITLE_WORD_SIZE 240

// ---- Per-thread error ring --------------------------------------------
namespace {

thread_local std::string g_last_error;

void set_error(const std::string& msg) { g_last_error = msg; }

// ---- Accent removal + lowercasing (mirrors libzim's removeAccents) ----
//
// Two rules:
//   - libzim: "Lower; NFD; [:M:] remove; NFC" — strips every combining
//     mark, regardless of script. Identical to libzim. Fragments Thai
//     vowel-sign sequences (U+0E30-0E3A) and Indic vowel marks; the
//     downstream tokeniser then produces consonant-cluster tokens.
//   - latin:  same shape, but only the four "Combining Diacritical
//     Marks*" blocks (used by Latin/IPA/symbols) are stripped. Indic /
//     Thai / Arabic vowel marks survive.
//
// The transliterator is built once per rule and reused across calls.

constexpr const char* RULE_LIBZIM =
    "Lower; NFD; [:M:] remove; NFC";
// Combining Diacritical Marks blocks: 0300-036F + 1AB0-1AFF (Extended)
// + 1DC0-1DFF (Supplement) + 20D0-20FF (for Symbols). These cover
// Latin/Greek/Cyrillic/IPA accent removal; Thai/Devanagari/Arabic vowel
// signs live elsewhere and pass through.
constexpr const char* RULE_LATIN_ONLY =
    "Lower; NFD; "
    "[\\u0300-\\u036F\\u1AB0-\\u1AFF\\u1DC0-\\u1DFF\\u20D0-\\u20FF] remove; "
    "NFC";

icu::Transliterator* build_translit(const std::string& rule_id) {
    UErrorCode status = U_ZERO_ERROR;
    const char* rule = (rule_id == "latin") ? RULE_LATIN_ONLY : RULE_LIBZIM;
    // Use createInstance (compound-ID parser), exactly like libzim.
    // createFromRules(...) takes a different rule grammar that doesn't
    // parse "Lower; NFD; [...] remove; NFC" the way we want.
    icu::UnicodeString id(rule, "UTF-8");
    icu::Transliterator* t =
        icu::Transliterator::createInstance(id, UTRANS_FORWARD, status);
    if (U_FAILURE(status)) return nullptr;
    return t;
}

std::string remove_accents_lower_with(icu::Transliterator* t, const std::string& s) {
    if (s.empty() || !t) return s;
    icu::UnicodeString us = icu::UnicodeString::fromUTF8(s);

    constexpr int32_t BATCH = 4 * 1024;
    if (us.length() <= BATCH) {
        t->transliterate(us);
        std::string out;
        us.toUTF8String(out);
        return out;
    }

    std::string out;
    int32_t pos = 0;
    icu::UnicodeString chunk;
    while (pos < us.length()) {
        int32_t end = us.getChar32Limit(pos + BATCH);
        int32_t len = end - pos;
        chunk.remove();
        us.extract(pos, len, chunk);
        t->transliterate(chunk);
        chunk.toUTF8String(out);
        pos += len;
    }
    return out;
}

// Default-rule accessor used by paths that pre-date per-builder
// configurability (e.g. parse_html). They get the libzim rule, which
// is the safest "match what kiwix does" default.
icu::Transliterator* get_default_translit() {
    static std::unique_ptr<icu::Transliterator> t(build_translit("libzim"));
    return t.get();
}

std::string remove_accents_lower(const std::string& s) {
    return remove_accents_lower_with(get_default_translit(), s);
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

struct XbBuilder {
    Xapian::WritableDatabase db;
    std::string tmp_path;
    std::string final_path;
    std::string language;          // ISO-639-3 (stored in metadata)
    std::string default_stemmer;   // "" = no stemming
    std::string accent_rule;       // "libzim" | "latin"
    Xapian::SimpleStopper stopper; // populated from stopwords text
    int mode;                      // 0=title, 1=fulltext
    bool empty = true;
    std::mutex db_mu;              // matches libzim's s_dbaccessLock
    // Per-language stemmer cache. Built lazily; reads + insertions
    // are mutex-protected.
    std::map<std::string, Xapian::Stem> stem_cache;
    // Owned transliterator for the chosen accent rule. The default
    // rule's transliterator comes from get_default_translit();
    // builders that pick "latin" hold their own copy.
    std::unique_ptr<icu::Transliterator> translit;
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
        auto* b = new XbBuilder;
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
        if (b->accent_rule == "latin") {
            b->translit.reset(build_translit("latin"));
        }
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
        unsigned int flags = Xapian::DB_CREATE_OR_OVERWRITE;
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

        return b;
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
// override wins, otherwise fall back to the builder default. Cache
// the resulting Stem objects (creation isn't free for every call).
//
// Caller must hold `b->db_mu` while calling — we reuse the same mutex
// to keep the cache map racing-free without introducing a second
// lock.
static const Xapian::Stem* resolve_stem(XbBuilder* b, const char* lang_override) {
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

    auto it = b->stem_cache.find(key);
    if (it != b->stem_cache.end()) return &it->second;

    // Try literal first.
    try {
        auto [iter, _] = b->stem_cache.emplace(key, Xapian::Stem(key));
        return &iter->second;
    } catch (...) {}

    // Fall back to ICU language-code resolution (handles "eng" -> "en").
    std::string resolved = stemmer_lang_for(key);
    if (!resolved.empty() && resolved != key) {
        try {
            auto [iter, _] = b->stem_cache.emplace(key, Xapian::Stem(resolved));
            return &iter->second;
        } catch (...) {}
    }
    // Cache the failure as an empty Stem-less entry so we don't
    // reattempt construction on every call. We do this by NOT
    // inserting; lookups for the same key will retry. That's a
    // minor inefficiency but keeps the map's invariant simple
    // (every entry is a valid stemmer).
    return nullptr;
}

extern "C" int xb_add_title(XbBuilder* b,
                            const char* path,
                            const char* title,
                            const char* target_path,
                            const char* lang_override) {
    if (!b || b->mode != 0) {
        set_error("xb_add_title: builder is not in title mode");
        return -1;
    }
    try {
        const std::string path_s = path ? path : "";
        const std::string title_s = title ? title : "";
        const std::string target_s = target_path ? target_path : "";

        Xapian::TermGenerator indexer;
        indexer.set_max_word_length(MAX_INDEXABLE_TITLE_WORD_SIZE);
        indexer.set_flags(Xapian::TermGenerator::FLAG_CJK_NGRAM);
        // Stemmer resolution touches b->stem_cache, guarded by db_mu.
        std::lock_guard<std::mutex> lock(b->db_mu);
        const Xapian::Stem* s = resolve_stem(b, lang_override);
        if (s) {
            indexer.set_stemmer(*s);
            indexer.set_stemming_strategy(
                Xapian::TermGenerator::STEM_SOME);
        }

        Xapian::Document doc;
        std::string full_path = "C/" + path_s;
        doc.set_data(full_path);
        indexer.set_document(doc);

        icu::Transliterator* t = b->translit ? b->translit.get() : get_default_translit();
        std::string unaccented = remove_accents_lower_with(t, title_s);

        doc.add_value(0, title_s);
        doc.add_value(1, target_s.empty() ? path_s : target_s);

        if (!unaccented.empty()) {
            std::string anchored = ANCHOR_TERM + unaccented;
            indexer.index_text(anchored, 1);
            if (anchored.size() >=
                size_of_indexed_text(doc) + MAX_INDEXABLE_TITLE_WORD_SIZE) {
                set_error("title indexing: too much data lost");
                return -1;
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

        // db_mu is already held above for stemmer resolution; reuse
        // it for the actual add_document call.
        b->db.add_document(doc);
        b->empty = false;
        return 0;
    } catch (const std::exception& e) {
        set_error(std::string("xb_add_title: ") + e.what());
        return -1;
    } catch (...) {
        set_error("xb_add_title: unknown error");
        return -1;
    }
}

extern "C" int xb_add_fulltext(XbBuilder* b,
                               const char* path,
                               const char* title,
                               const char* content,
                               size_t content_len,
                               const char* keywords,
                               uint32_t word_count,
                               int has_geo,
                               double latitude,
                               double longitude,
                               const char* lang_override) {
    if (!b || b->mode != 1) {
        set_error("xb_add_fulltext: builder is not in fulltext mode");
        return -1;
    }
    try {
        const std::string path_s = path ? path : "";
        const std::string title_s = title ? title : "";
        const std::string content_s = content ? std::string(content, content_len) : "";
        const std::string keywords_s = keywords ? keywords : "";

        Xapian::TermGenerator indexer;
        indexer.set_flags(Xapian::TermGenerator::FLAG_CJK_NGRAM);
        std::lock_guard<std::mutex> lock(b->db_mu);
        const Xapian::Stem* s = resolve_stem(b, lang_override);
        if (s) {
            indexer.set_stemmer(*s);
            indexer.set_stemming_strategy(
                Xapian::TermGenerator::STEM_ALL);
        }
        indexer.set_stopper(&b->stopper);
        indexer.set_stopper_strategy(Xapian::TermGenerator::STOP_ALL);

        Xapian::Document doc;
        indexer.set_document(doc);
        std::string full_path = "C/" + path_s;
        doc.set_data(full_path);
        doc.add_value(0, title_s);
        {
            std::ostringstream os;
            os << word_count;
            doc.add_value(1, os.str());
        }
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

        // db_mu already held above for stemmer resolution.
        b->db.add_document(doc);
        b->empty = false;
        return 0;
    } catch (const std::exception& e) {
        set_error(std::string("xb_add_fulltext: ") + e.what());
        return -1;
    } catch (...) {
        set_error("xb_add_fulltext: unknown error");
        return -1;
    }
}

extern "C" int xb_finalize(XbBuilder* b) {
    if (!b) return -1;
    try {
        b->db.commit();
        b->db.compact(b->final_path,
                      Xapian::DBCOMPACT_SINGLE_FILE |
                          Xapian::Compactor::FULL);
        b->db.close();
        return 0;
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

// Cached transliterator for `xb_parse_html`. The libzim default is
// reused across calls; the latin rule is built once on first use.
icu::Transliterator* get_translit_for_rule(const std::string& rule) {
    if (rule == "latin") {
        static std::unique_ptr<icu::Transliterator> t(build_translit("latin"));
        return t.get();
    }
    return get_default_translit();
}

extern "C" XbParsedDoc* xb_parse_html(const char* html, size_t len, const char* accent_rule) {
    try {
        zim::MyHtmlParser parser;
        std::string body(html ? html : "", html ? len : 0);
        try {
            parser.parse_html(body, "UTF-8", true);
        } catch (...) {
            // MyHtmlParser uses exceptions for control flow on certain
            // tags (e.g. <noindex>); the partial state in `dump` is
            // still valid, matching libzim's behaviour.
        }
        auto* p = new XbParsedDoc;
        p->word_count = count_words(parser.dump);
        p->indexing_allowed =
            !parser.dump.empty() && parser.indexing_allowed &&
            (parser.dump.find("NOINDEX") == std::string::npos);
        p->has_geo = parser.has_geoPosition;
        p->latitude = parser.latitude;
        p->longitude = parser.longitude;
        const std::string rule = accent_rule ? accent_rule : "";
        icu::Transliterator* t = get_translit_for_rule(rule);
        p->content = remove_accents_lower_with(t, parser.dump);
        p->keywords = remove_accents_lower_with(t, parser.keywords);
        return p;
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
extern "C" const char* xb_pd_keywords(const XbParsedDoc* p) {
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
