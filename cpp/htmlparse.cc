/* htmlparse.cc: simple HTML parser for omega indexer
 *
 * Copyright 1999,2000,2001 BrightStation PLC
 * Copyright 2001 Ananova Ltd
 * Copyright 2002,2006,2007,2008 Olly Betts
 *
 * This program is free software; you can redistribute it and/or
 * modify it under the terms of the GNU General Public License as
 * published by the Free Software Foundation; either version 2 of the
 * License, or (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License
 * along with this program; if not, write to the Free Software
 * Foundation, Inc., 51 Franklin St, Fifth Floor, Boston, MA  02110-1301
 * USA
 */

// #include <config.h>

#include "htmlparse.h"

#include <xapian.h>

// #include "utf8convert.h"

#include <algorithm>
#include <charconv>
#include <mutex>
#include <string_view>

#include <ctype.h>
#include <cstring>

using namespace std;

inline void
lowercase_string(string &str)
{
    for (string::iterator i = str.begin(); i != str.end(); ++i) {
	*i = tolower(static_cast<unsigned char>(*i));
    }
}

map<string, unsigned int, std::less<>> zim::HtmlParser::named_ents;
static std::mutex sInitLock;

inline static bool
p_notdigit(char c)
{
    return !isdigit(static_cast<unsigned char>(c));
}

inline static bool
p_notxdigit(char c)
{
    return !isxdigit(static_cast<unsigned char>(c));
}

inline static bool
p_notalnum(char c)
{
    return !isalnum(static_cast<unsigned char>(c));
}

inline static bool
p_notwhitespace(char c)
{
    return !isspace(static_cast<unsigned char>(c));
}

inline static bool
p_nottag(char c)
{
    return !isalnum(static_cast<unsigned char>(c)) &&
	c != '.' && c != '-' && c != ':'; // ':' for XML namespaces.
}

inline static bool
p_whitespacegt(char c)
{
    return isspace(static_cast<unsigned char>(c)) || c == '>';
}

inline static bool
p_whitespaceeqgt(char c)
{
    return isspace(static_cast<unsigned char>(c)) || c == '=' || c == '>';
}

bool
zim::HtmlParser::get_parameter(const string & param, string & value)
{
    map<string, string>::const_iterator i = parameters.find(param);
    if (i == parameters.end()) return false;
    value = i->second;
    return true;
}

zim::HtmlParser::HtmlParser()
{
    static const struct ent { const char *n; unsigned int v; } ents[] = {
#include "namedentities.h"
	{ NULL, 0 }
    };
    std::lock_guard<std::mutex> l(sInitLock);
    if (named_ents.empty()) {
	const struct ent *i = ents;
	while (i->n) {
	    named_ents[string(i->n)] = i->v;
	    ++i;
	}
    }
}

void
zim::HtmlParser::decode_entities(string &s)
{
    // Decode into the already-consumed prefix. Every replacement is no longer
    // than its source, so no allocation or repeated suffix shifting is needed.
    size_t read = 0, write = 0;
    while (read < s.size()) {
        const size_t amp = s.find('&', read);
        const size_t prefix_end = amp == string::npos ? s.size() : amp;
        const size_t count = prefix_end - read;
        if (count) std::memmove(s.data() + write, s.data() + read, count);
        write += count;
        if (amp == string::npos) break;

        unsigned int val = 0;
        size_t p = amp + 1;
        size_t end = p;
        if (p < s.size() && s[p] == '#') {
            ++p;
            int base = 10;
            if (p < s.size() && (s[p] == 'x' || s[p] == 'X')) {
                ++p;
                base = 16;
            }
            end = p;
            while (end < s.size() &&
                   !(base == 16 ? p_notxdigit(s[end]) : p_notdigit(s[end]))) ++end;
            const auto result = std::from_chars(s.data() + p, s.data() + end, val, base);
            if (result.ec == std::errc::result_out_of_range ||
                val > 0x10ffff || (val >= 0xd800 && val <= 0xdfff)) {
                val = 0xfffd;
            }
        } else {
            while (end < s.size() && !p_notalnum(s[end])) ++end;
            auto entity = named_ents.find(std::string_view(s.data() + p, end - p));
            if (entity != named_ents.end()) val = entity->second;
        }
        if (end < s.size() && s[end] == ';') ++end;
        if (val) {
            if (val < 0x80) {
                s[write++] = static_cast<char>(val);
            } else {
                char encoded[4];
                const unsigned length = Xapian::Unicode::nonascii_to_utf8(val, encoded);
                std::memcpy(s.data() + write, encoded, length);
                write += length;
            }
            read = end;
        } else {
            // Unknown references remain literal; an '&' later in their text
            // can still begin a recognized reference.
            s[write++] = '&';
            read = amp + 1;
        }
    }
    s.resize(write);
}

void
zim::HtmlParser::parse_html(const string &body)
{
    raw_text_tag.clear();

    parameters.clear();
    string::const_iterator start = body.begin();

    while (true) {
        if (!raw_text_tag.empty()) {
            // Script/style bodies are raw text. Only their matching end tag
            // can resume HTML parsing; '</body>' in JS/CSS is not a body end.
            auto closing = start;
            while ((closing = find(closing, body.end(), '<')) != body.end()) {
                const auto remaining = body.end() - closing;
                if (static_cast<size_t>(remaining) > raw_text_tag.size() + 2 &&
                    closing[1] == '/' &&
                    std::equal(raw_text_tag.begin(), raw_text_tag.end(), closing + 2,
                        [](char expected, char actual) {
                            return expected == tolower(static_cast<unsigned char>(actual));
                        })) {
                    const unsigned char after = closing[raw_text_tag.size() + 2];
                    if (isspace(after) || after == '/' || after == '>') break;
                }
                ++closing;
            }
            if (closing == body.end()) break;
            start = closing;
            raw_text_tag.clear();
        }
	// Skip through until we find an HTML tag, a comment, or the end of
	// document.  Ignore isolated occurrences of `<' which don't start
	// a tag or comment.
	string::const_iterator p = start;
	while (true) {
	    p = find(p, body.end(), '<');
	    if (p == body.end()) break;
	    if (p + 1 == body.end()) {
		p = body.end();
		break;
	    }
	    unsigned char ch = *(p + 1);

	    // Tag, closing tag, or comment (or SGML declaration).
	    if (isalpha(ch) || ch == '/' || ch == '!') break;

	    if (ch == '?') {
		// PHP code or XML declaration.
		// XML declaration is only valid at the start of the first line.
		// FIXME: need to deal with BOMs...
		if (p != body.begin() || body.size() < 20) break;

		// XML declaration looks something like this:
		// <?xml version="1.0" encoding="UTF-8"?>
		if (p[2] != 'x' || p[3] != 'm' || p[4] != 'l') break;
		if (strchr(" \t\r\n", p[5]) == NULL) break;

		string::const_iterator decl_end = find(p + 6, body.end(), '?');
		if (decl_end == body.end()) break;

		// Default charset for XML is UTF-8.
		charset = "UTF-8";

		string decl(p + 6, decl_end);
		size_t enc = decl.find("encoding");
		if (enc == string::npos) break;

		enc = decl.find_first_not_of(" \t\r\n", enc + 8);
		if (enc == string::npos || enc == decl.size()) break;

		if (decl[enc] != '=') break;
		
		enc = decl.find_first_not_of(" \t\r\n", enc + 1);
		if (enc == string::npos || enc == decl.size()) break;

		if (decl[enc] != '"' && decl[enc] != '\'') break;

		char quote = decl[enc++];
		size_t enc_end = decl.find(quote, enc);

		if (enc != string::npos)
		    charset = decl.substr(enc, enc_end - enc);

		break;
	    }
	    p++;
	}

	// Process text up to start of tag.
	if (p > start) {
	    string text = body.substr(start - body.begin(), p - start);
	    // convert_to_utf8(text, charset);
	    decode_entities(text);
	    process_text(text);
	}

	if (p == body.end()) break;

	start = p + 1;

	if (start == body.end()) break;

	if (*start == '!') {
	    if (++start == body.end()) break;
	    if (++start == body.end()) break;
	    // comment or SGML declaration
	    if (*(start - 1) == '-' && *start == '-') {
		++start;
		string::const_iterator close = find(start, body.end(), '>');
		// An unterminated comment swallows rest of document
		// (like Netscape, but unlike MSIE IIRC)
		if (close == body.end()) break;

		p = close;
		// look for -->
		while (p != body.end() && (*(p - 1) != '-' || *(p - 2) != '-'))
		    p = find(p + 1, body.end(), '>');

		if (p != body.end()) {
		    // Check for htdig's "ignore this bit" comments.
		    if (p - start == 15 && string(start, p - 2) == "htdig_noindex") {
			string::size_type i;
			i = body.find("<!--/htdig_noindex-->", p + 1 - body.begin());
			if (i == string::npos) break;
			start = body.begin() + i + 21;
			continue;
		    }
		    // If we found --> skip to there.
		    start = p;
		} else {
		    // Otherwise skip to the first > we found (as Netscape does).
		    start = close;
		}
	    } else {
		// just an SGML declaration, perhaps giving the DTD - ignore it
		start = find(start - 1, body.end(), '>');
		if (start == body.end()) break;
	    }
	    ++start;
	} else if (*start == '?') {
	    if (++start == body.end()) break;
	    // PHP - swallow until ?> or EOF
	    start = find(start + 1, body.end(), '>');

	    // look for ?>
	    while (start != body.end() && *(start - 1) != '?')
		start = find(start + 1, body.end(), '>');

	    // unterminated PHP swallows rest of document (rather arbitrarily
	    // but it avoids polluting the database when things go wrong)
	    if (start != body.end()) ++start;
	} else {
	    // opening or closing tag
	    int closing = 0;

	    if (*start == '/') {
		closing = 1;
		start = find_if(start + 1, body.end(), p_notwhitespace);
	    }

	    p = start;
	    start = find_if(start, body.end(), p_nottag);
	    string tag = body.substr(p - body.begin(), start - p);
	    // convert tagname to lowercase
	    lowercase_string(tag);

	    if (closing) {
		closing_tag(tag);

		/* ignore any bogus parameters on closing tags */
		p = find(start, body.end(), '>');
		if (p == body.end()) break;
		start = p + 1;
	    } else {
		// FIXME: parse parameters lazily.
		while (start < body.end() && *start != '>') {
		    string name, value;

		    p = find_if(start, body.end(), p_whitespaceeqgt);

		    name.assign(body, start - body.begin(), p - start);

		    p = find_if(p, body.end(), p_notwhitespace);

		    start = p;
		    if (start != body.end() && *start == '=') {
			start = find_if(start + 1, body.end(), p_notwhitespace);
			if (start == body.end()) break;

			p = body.end();

			int quote = *start;
			if (quote == '"' || quote == '\'') {
			    start++;
			    p = find(start, body.end(), quote);
			}

			if (p == body.end()) {
			    // unquoted or no closing quote
			    p = find_if(start, body.end(), p_whitespacegt);
			}
			value.assign(body, start - body.begin(), p - start);
			start = find_if(p, body.end(), p_notwhitespace);

			if (!name.empty()) {
			    // convert parameter name to lowercase
			    lowercase_string(name);
			    // in case of multiple entries, use the first
			    // (as Netscape does)
			    parameters.insert(make_pair(name, value));
			}
		    }
		}
#if 0
		cout << "<" << tag;
		map<string, string>::const_iterator x;
		for (x = parameters.begin(); x != parameters.end(); x++) {
		    cout << " " << x->first << "=\"" << x->second << "\"";
		}
		cout << ">\n";
#endif
		opening_tag(tag);
		parameters.clear();

		if (tag == "script" || tag == "style") raw_text_tag = tag;

		if (start != body.end() && *start == '>') ++start;
	    }
	}
    }
}
