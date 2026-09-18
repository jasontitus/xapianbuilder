// Standalone C ABI regressions for ASan, UBSan, and LeakSanitizer.
#include "bridge.h"

#include <filesystem>
#include <iostream>
#include <limits>
#include <memory>
#include <stdexcept>
#include <string>
#include <string_view>

namespace fs = std::filesystem;
using Parsed = std::unique_ptr<XbParsedDoc, decltype(&xb_pd_free)>;
using Builder = std::unique_ptr<XbBuilder, decltype(&xb_builder_free)>;
using Doc = std::unique_ptr<XbDoc, decltype(&xb_doc_free)>;

static void require(bool condition, const std::string& message) {
    if (!condition) throw std::runtime_error(message);
}

static Parsed parse(std::string_view html) {
    Parsed parsed(xb_parse_html(html.data(), html.size(), "libzim"), xb_pd_free);
    require(bool(parsed), std::string("parsing failed: ") + xb_last_error());
    return parsed;
}

static std::string content(const Parsed& parsed) {
    size_t len = 0;
    const char* text = xb_pd_content(parsed.get(), &len);
    return std::string(text, len);
}

static void check_parse_cases() {
    auto truncated = parse("<body>Before &amp; after <");
    require(content(truncated) == "before & after <", "trailing angle lost text");
    for (const char* suffix : {"<p key=", "<p key=   ", "<!--", "</", "<script>unfinished"}) {
        auto parsed = parse(std::string("<body>Before") + suffix);
        require(content(parsed) == "before", std::string("truncated tag: ") + suffix);
    }

    auto entities = parse("<body>&#65; &#x1F600; &#4294967361; &#x100000041; "
                          "&#x110000; &#xD800; &#999999999999999999999999999999999999999; "
                          "&#x; &unknown;</body>");
    require(content(entities) == u8"a \U0001f600 \ufffd \ufffd \ufffd \ufffd \ufffd &#x; &unknown;",
            "numeric entity overflow or invalid Unicode scalar");

    const char nul_html[] = "<meta name='keywords' content='BEFORE\0AFTER &amp; END'><body>Text</body>";
    auto nul = parse(std::string_view(nul_html, sizeof(nul_html) - 1));
    size_t keyword_len = 0;
    const char* keywords = xb_pd_keywords(nul.get(), &keyword_len);
    const char expected_keywords[] = "before\0after & end";
    require(std::string_view(keywords, keyword_len) ==
                std::string_view(expected_keywords, sizeof(expected_keywords) - 1),
            "keywords truncated at embedded NUL");

    for (const char* geo : {";", " ; ", "bad;2", "1;2x", "1;2;3", "nan;0", "0;inf",
                            "1e999;0", "90.0000001;0", "0;-180.0000001"}) {
        auto parsed = parse(std::string("<meta name='geo.position' content='") + geo +
                            "'><body>Searchable</body>");
        require(!xb_pd_has_geo(parsed.get()), std::string("invalid geo accepted: ") + geo);
        require(xb_pd_indexing_allowed(parsed.get()) && content(parsed) == "searchable",
                "invalid optional geo damaged document");
    }
    struct CoordinateCase { const char* text; double latitude; double longitude; };
    for (const auto& geo : {CoordinateCase{"90;180", 90, 180},
                            CoordinateCase{"-90;-180", -90, -180},
                            CoordinateCase{" 1.25e1 ; -4.25 ", 12.5, -4.25}}) {
        auto parsed = parse(std::string("<meta name='geo.position' content='") + geo.text +
                            "'><body>Place</body>");
        require(xb_pd_has_geo(parsed.get()) && xb_pd_latitude(parsed.get()) == geo.latitude &&
                    xb_pd_longitude(parsed.get()) == geo.longitude,
                std::string("valid geo rejected: ") + geo.text);
    }
    auto duplicate = parse("<meta name='geo.position' content='12.5;-4.25'>"
                           "<meta name='geo.position' content='80;invalid'><body>Place</body>");
    require(xb_pd_latitude(duplicate.get()) == 12.5 && xb_pd_longitude(duplicate.get()) == -4.25,
            "invalid later geo corrupted valid coordinates");

    auto noindex = parse("<meta name='robots' content='noindex'><body>Excluded</body>");
    require(!xb_pd_indexing_allowed(noindex.get()), "robots noindex was lost");
    auto body_end = parse("<body>Visible</body>Excluded");
    require(content(body_end) == "visible", "body-end parser stop was lost");

    auto script = parse("<body>before<script>const x = '</body><!--<style>';</script>after</body>");
    auto style = parse("<body>before<style>x{content:'</body><script>'}</style>after</body>");
    require(content(script) == "beforeafter" && content(style) == "beforeafter",
            "raw text terminated visible body content");
    const std::string prefix(4095, 'a');
    require(content(parse("<body>" + prefix + u8"ΣA</body>")) == prefix + u8"σa",
            "lowercasing lost context across a batch");
    require(content(parse("<body>" + prefix + u8"\u1100\u1161</body>")) == prefix + u8"가",
            "normalization split a composing sequence");
    require(content(parse("<body>&amp;lt;&unknown;&bogus&amp;&#65;</body>")) ==
                "&lt;&unknown;&bogus&a", "in-place entity decoding changed semantics");

    Parsed invalid(xb_parse_html(nullptr, 1, "libzim"), xb_pd_free);
    require(!invalid, "null nonempty HTML buffer accepted");
    require(std::string(xb_last_error()).find("null HTML buffer") != std::string::npos,
            "parse failure lost its diagnostic");
}

int main(int argc, char** argv) {
    try {
        require(argc == 2, "usage: native-safety EXISTING_PRIVATE_WORKSPACE");
        const fs::path workspace(argv[1]);
        require(fs::is_directory(workspace), "workspace must be an existing private directory");
        fs::path owned;
        for (unsigned attempt = 0; attempt != 1024; ++attempt) {
            const auto candidate = workspace / ("native-safety-" + std::to_string(attempt));
            if (fs::create_directory(candidate)) {
                owned = candidate;
                break;
            }
        }
        require(!owned.empty(), "could not create an exclusive harness directory");
        const auto missing = (owned / "missing-parent" / "db").string();
        const auto staged = (owned / "staged.xapian").string();
        for (unsigned iteration = 0; iteration != 128; ++iteration) {
            Builder failed(xb_builder_new(missing.c_str(), staged.c_str(), "eng", "", "none",
                                           "libzim", 0, 1), xb_builder_free);
            require(!failed, "constructor unexpectedly created a missing parent");
            const std::string error = xb_last_error();
            require(error.find("Error") != std::string::npos &&
                        error.find("unknown error") == std::string::npos,
                    "constructor lost Xapian error description: " + error);
            check_parse_cases();
        }

        const auto database = (owned / "database").string();
        Builder builder(xb_builder_new(database.c_str(), staged.c_str(), "eng", "", "none",
                                        "libzim", 0, 1), xb_builder_free);
        require(bool(builder), std::string("builder creation failed: ") + xb_last_error());
        for (double latitude : {91.0, std::numeric_limits<double>::quiet_NaN(),
                                std::numeric_limits<double>::infinity()}) {
            Doc invalid(xb_prepare_fulltext(builder.get(), "place", "Place", "body", 4,
                                             "", 0, 1, 1, latitude, 0, ""), xb_doc_free);
            require(!invalid && xb_builder_is_empty(builder.get()), "invalid direct geo accepted");
            require(std::string(xb_last_error()).find("InvalidArgumentError") != std::string::npos,
                    "direct geo failure lost Xapian error type");
        }
        Doc valid(xb_prepare_fulltext(builder.get(), "place", "Place", "body", 4,
                                      "", 0, 1, 1, 90, -180, ""), xb_doc_free);
        require(bool(valid), std::string("valid document failed: ") + xb_last_error());
        require(xb_add_doc(builder.get(), valid.get()) == 0 && !xb_builder_is_empty(builder.get()),
                "builder unusable after failed preparation");
        const int finalized = xb_finalize(builder.get());
        require(finalized == 0, std::string("finalization failed: ") + xb_last_error());
        require(fs::is_regular_file(staged), "staged database was not created");
        std::cout << "native safety regressions passed (128 failure/parse cycles)\n";
        return 0;
    } catch (const std::exception& error) {
        std::cerr << "native safety regression: " << error.what() << '\n';
        return 1;
    }
}
