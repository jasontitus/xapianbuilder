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
// Rule string is identical to libzim's: "Lower; NFD; [:M:] remove; NFC".
// The transliterator is created lazily and reused across calls.

icu::Transliterator* get_translit() {
    static UErrorCode status = U_ZERO_ERROR;
    static std::unique_ptr<icu::Transliterator> t(
        icu::Transliterator::createInstance(
            "Lower; NFD; [:M:] remove; NFC", UTRANS_FORWARD, status));
    return t.get();
}

std::string remove_accents_lower(const std::string& s) {
    if (s.empty() || !get_translit()) return s;
    icu::UnicodeString us = icu::UnicodeString::fromUTF8(s);

    constexpr int32_t BATCH = 4 * 1024;
    if (us.length() <= BATCH) {
        get_translit()->transliterate(us);
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
        get_translit()->transliterate(chunk);
        chunk.toUTF8String(out);
        pos += len;
    }
    return out;
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
    std::string stemmer_language;  // resolved stemmer string ("" = none)
    Xapian::SimpleStopper stopper; // populated from stopwords text
    int mode;                      // 0=title, 1=fulltext
    bool empty = true;
    std::mutex db_mu;              // matches libzim's s_dbaccessLock
};

extern "C" XbBuilder* xb_builder_new(const char* tmp_path,
                                     const char* final_path,
                                     const char* language_iso6393,
                                     const char* stopwords_text,
                                     const char* stemmer_override,
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
            b->stemmer_language = "";
        } else if (!ovr.empty()) {
            b->stemmer_language = ovr;
        } else {
            b->stemmer_language = stemmer_lang_for(b->language);
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

extern "C" int xb_add_title(XbBuilder* b,
                            const char* path,
                            const char* title,
                            const char* target_path) {
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
        if (!b->stemmer_language.empty()) {
            try {
                Xapian::Stem s(b->stemmer_language);
                indexer.set_stemmer(s);
                indexer.set_stemming_strategy(
                    Xapian::TermGenerator::STEM_SOME);
            } catch (...) {}
        }

        Xapian::Document doc;
        std::string full_path = "C/" + path_s;
        doc.set_data(full_path);
        indexer.set_document(doc);

        std::string unaccented = remove_accents_lower(title_s);

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

        std::lock_guard<std::mutex> lock(b->db_mu);
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
                               double longitude) {
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
        if (!b->stemmer_language.empty()) {
            try {
                Xapian::Stem s(b->stemmer_language);
                indexer.set_stemmer(s);
                indexer.set_stemming_strategy(
                    Xapian::TermGenerator::STEM_ALL);
            } catch (...) {}
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

        std::lock_guard<std::mutex> lock(b->db_mu);
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

extern "C" XbParsedDoc* xb_parse_html(const char* html, size_t len) {
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
        p->content = remove_accents_lower(parser.dump);
        p->keywords = remove_accents_lower(parser.keywords);
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
