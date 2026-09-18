// SPDX-License-Identifier: GPL-3.0-or-later
// Original comparison tooling, using only installed libzim/Xapian public APIs.
// The fixture is intentionally tiny: materializing a canonical semantic snapshot
// keeps diagnostics legible and makes asynchronous docid allocation irrelevant.
#include <zim/archive.h>
#include <zim/blob.h>
#include <zim/entry.h>
#include <zim/item.h>
#include <zim/version.h>
#include <zim/writer/contentProvider.h>
#include <zim/writer/creator.h>
#include <zim/writer/item.h>
#include <xapian.h>

#include <filesystem>
#include <fstream>
#include <iostream>
#include <iterator>
#include <limits>
#include <map>
#include <sstream>
#include <set>
#include <stdexcept>
#include <string>
#include <vector>

namespace fs = std::filesystem;

std::string hex(const std::string& bytes) {
    static constexpr char digits[] = "0123456789abcdef";
    std::string result;
    result.reserve(bytes.size() * 2);
    for (unsigned char byte : bytes) {
        result += digits[byte >> 4];
        result += digits[byte & 15];
    }
    return result;
}

std::string load(const fs::path& path) {
    std::ifstream input(path, std::ios::binary);
    if (!input) throw std::runtime_error("cannot open " + path.string());
    std::string bytes((std::istreambuf_iterator<char>(input)), {});
    if (input.bad()) throw std::runtime_error("cannot read " + path.string());
    return bytes;
}

void save(const fs::path& path, const zim::Blob& blob) {
    if (fs::exists(fs::symlink_status(path))) {
        throw std::runtime_error("refusing existing output " + path.string());
    }
    std::ofstream output(path, std::ios::binary);
    output.exceptions(std::ios::failbit | std::ios::badbit);
    output.write(blob.data(), blob.size());
    output.close();
}

void create(const fs::path& manifest, const fs::path& directory) {
    // A fresh directory also protects libzim's internal sidecar filenames.
    if (!fs::create_directory(directory)) {
        throw std::runtime_error("creation directory must not exist: " + directory.string());
    }
    const auto archive_path = directory / "upstream.zim";
    zim::writer::Creator creator;
    creator.configVerbose(false).configCompression(zim::Compression::Zstd)
        .configIndexing(true, "eng").configNbWorkers(1);
    creator.startZimCreation(archive_path.string());
    creator.addMetadata("Language", "eng");
    creator.addMetadata("Title", "xapianbuilder original comparison fixture");
    creator.addMetadata("Creator", "xapianbuilder contributors");
    creator.addMetadata("Date", "2026-01-01");
    const zim::writer::Hints hints{{zim::writer::FRONT_ARTICLE, 1}};
    std::istringstream input(load(manifest));
    std::string line;
    unsigned count = 0;
    while (std::getline(input, line)) {
        std::vector<std::string> fields;
        std::size_t begin = 0;
        for (;;) {
            auto end = line.find('\t', begin);
            fields.push_back(line.substr(begin, end == std::string::npos ? end : end - begin));
            if (end == std::string::npos) break;
            begin = end + 1;
        }
        if (fields.size() != 5 || fields[0].empty()) {
            throw std::runtime_error("invalid fixture manifest row");
        }
        if (!fields[4].empty()) {
            creator.addRedirection(fields[0], fields[1], fields[4], hints);
        } else {
            auto body = load(manifest.parent_path() / fields[3]);
            creator.addItem(zim::writer::StringItem::create(
                fields[0], fields[2], fields[1], hints, body));
        }
        ++count;
    }
    if (!count) throw std::runtime_error("empty fixture manifest");
    creator.setMainPath("orchard");
    creator.finishZimCreation();

    zim::Archive archive(archive_path.string());
    std::map<std::string, unsigned> extracted{{"title/xapian", 0}, {"fulltext/xapian", 0}};
    // Numeric path indexes cover all entries (including X), unlike the public
    // string lookup on modern archives, which intentionally searches C only.
    for (zim::entry_index_type i = 0; i < archive.getAllEntryCount(); ++i) {
        const auto entry = archive.getEntryByPath(i);
        auto found = extracted.find(entry.getPath());
        if (found == extracted.end() || entry.isRedirect()) continue;
        const auto item = entry.getItem();
        if (item.getMimetype() != "application/octet-stream+xapian") continue;
        if (++found->second != 1) throw std::runtime_error("duplicate index entry");
        const auto name = found->first.substr(0, found->first.find('/'));
        save(directory / (name + ".xapian"), item.getData());
    }
    for (const auto& pair : extracted) {
        if (pair.second != 1) throw std::runtime_error("upstream omitted index: " + pair.first);
    }
    std::cout << "Created upstream fixture from " << count << " records\n";
}

void snapshot(const std::string& path) {
    Xapian::Database db(path);
    std::cout << "COUNT " << db.get_doccount() << '\n';
    for (auto key = db.metadata_keys_begin(); key != db.metadata_keys_end(); ++key) {
        std::cout << "META " << hex(*key) << ' ' << hex(db.get_metadata(*key)) << '\n';
    }
    // Without termlists, Document::values_begin() can be empty even when
    // get_value(slot) returns stored values. The metadata defines the index's
    // value schema; query those slots explicitly instead of silently omitting
    // the title, redirect target, word count, and coordinates from comparison.
    std::set<Xapian::valueno> slots;
    std::istringstream schema(db.get_metadata("valuesmap"));
    std::string field;
    while (std::getline(schema, field, ';')) {
        const auto colon = field.find(':');
        if (colon == std::string::npos || colon == 0 || colon + 1 == field.size()) {
            throw std::runtime_error("invalid valuesmap field");
        }
        const auto number = field.substr(colon + 1);
        if (number.find_first_not_of("0123456789") != std::string::npos) {
            throw std::runtime_error("invalid valuesmap slot");
        }
        const auto slot = std::stoull(number);
        if (slot > std::numeric_limits<Xapian::valueno>::max()) {
            throw std::runtime_error("valuesmap slot out of range");
        }
        slots.insert(static_cast<Xapian::valueno>(slot));
    }
    if (slots.empty()) throw std::runtime_error("missing valuesmap schema");
    std::map<std::string, Xapian::docid> documents;
    std::map<Xapian::docid, std::string> paths;
    for (auto posting = db.postlist_begin(""); posting != db.postlist_end(""); ++posting) {
        auto data = db.get_document(*posting).get_data();
        if (!documents.emplace(data, *posting).second) {
            throw std::runtime_error("duplicate document identity: " + hex(data));
        }
        paths.emplace(*posting, std::move(data));
    }
    for (const auto& pair : documents) {
        const auto data = hex(pair.first);
        const auto document = db.get_document(pair.second);
        std::cout << "DOC " << data << ' ' << db.get_doclength(pair.second) << '\n';
        for (const auto slot : slots) {
            const auto value = document.get_value(slot);
            if (!value.empty()) {
                std::cout << "VALUE " << data << ' ' << slot << ' ' << hex(value) << '\n';
            }
        }
    }
    for (auto term = db.allterms_begin(); term != db.allterms_end(); ++term) {
        const auto word = *term;
        std::cout << "TERM " << hex(word) << ' ' << db.get_termfreq(word)
                  << ' ' << db.get_collection_freq(word) << '\n';
        std::map<std::string, std::string> postings;
        for (auto posting = db.postlist_begin(word); posting != db.postlist_end(word); ++posting) {
            std::ostringstream details;
            details << posting.get_wdf();
            for (auto position = db.positionlist_begin(*posting, word);
                 position != db.positionlist_end(*posting, word); ++position) {
                details << ' ' << *position;
            }
            postings.emplace(paths.at(*posting), details.str());
        }
        for (const auto& pair : postings) {
            std::cout << "POST " << hex(word) << ' ' << hex(pair.first) << ' ' << pair.second << '\n';
        }
    }
}

int main(int argc, char** argv) {
    try {
        if (argc == 2 && std::string(argv[1]) == "versions") {
            for (const auto& pair : zim::getVersions()) {
                std::cout << pair.first << ' ' << pair.second << '\n';
            }
            std::cout << "Xapian runtime " << Xapian::version_string() << '\n';
            std::cout << "Xapian headers " << XAPIAN_VERSION << '\n';
        } else if (argc == 4 && std::string(argv[1]) == "create") {
            create(argv[2], argv[3]);
        } else if (argc == 3 && std::string(argv[1]) == "snapshot") {
            snapshot(argv[2]);
        } else {
            throw std::runtime_error("usage: upstream-helper create MANIFEST NEW_DIRECTORY | snapshot DATABASE");
        }
        if (!std::cout) throw std::runtime_error("cannot write result");
        return 0;
    } catch (const Xapian::Error& error) {
        std::cerr << "Xapian: " << error.get_description() << '\n';
    } catch (const std::exception& error) {
        std::cerr << error.what() << '\n';
    }
    return 2;
}
