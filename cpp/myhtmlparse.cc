/* myhtmlparse.cc: subclass of HtmlParser for extracting text.
 *
 * Copyright 1999,2000,2001 BrightStation PLC
 * Copyright 2002,2003,2004,2006,2007,2008 Olly Betts
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

/*
 * Local modifications for xapianbuilder (upstream notice retained above):
 * Imported from libzim on 2026-04-27.
 * 2026-09-18: replaced unchecked geo parsing with classic-locale, complete-
 * consumption, finite-value and coordinate-range checks; publish coordinates
 * only when both are valid.
 * 2026-09-19: added this modification notice. See UPSTREAM.md for provenance.
 */

// #include <config.h>

#include "myhtmlparse.h"

// #include "utf8convert.h"

#include <ctype.h>
#include <cmath>
#include <locale>
#include <sstream>
#include <string.h>

inline void lowercase_string(string &str) {
  for (string::iterator i = str.begin(); i != str.end(); ++i) {
    *i = tolower(static_cast<unsigned char>(*i));
  }
}

void zim::MyHtmlParser::parse_html(const string &text, const string &charset_,
                                   bool charset_from_meta_) {
  charset = charset_;
  charset_from_meta = charset_from_meta_;
  HtmlParser::parse_html(text);
}

void zim::MyHtmlParser::process_text(const string &text) {
  if (!text.empty() && !in_script_tag && !in_style_tag) {
    string::size_type b = text.find_first_not_of(WHITESPACE);
    if (b)
      pending_space = true;
    while (b != string::npos) {
      if (pending_space && !dump.empty())
        dump += ' ';
      string::size_type e = text.find_first_of(WHITESPACE, b);
      pending_space = (e != string::npos);
      if (!pending_space) {
        dump.append(text.data() + b, text.size() - b);
        return;
      }
      dump.append(text.data() + b, e - b);
      b = text.find_first_not_of(WHITESPACE, e + 1);
    }
  }
}

// Geo metadata is optional: reject malformed values without stopping parsing.
static bool parse_coordinate(const std::string& text, double limit, double& value) {
  std::istringstream stream(text);
  stream.imbue(std::locale::classic());
  double parsed = 0;
  if (!(stream >> parsed) || !std::isfinite(parsed) ||
      parsed < -limit || parsed > limit)
    return false;
  stream >> std::ws;
  if (!stream.eof())
    return false;
  value = parsed;
  return true;
}

void zim::MyHtmlParser::opening_tag(const string &tag) {
  if (tag.empty())
    return;
  switch (tag[0]) {
  case 'a':
    if (tag == "address")
      pending_space = true;
    break;
  case 'b':
    if (tag == "body") {
      dump.resize(0);
      break;
    }
    if (tag == "blockquote" || tag == "br")
      pending_space = true;
    break;
  case 'c':
    if (tag == "center")
      pending_space = true;
    break;
  case 'd':
    if (tag == "dd" || tag == "dir" || tag == "div" || tag == "dl" ||
        tag == "dt")
      pending_space = true;
    break;
  case 'e':
    if (tag == "embed")
      pending_space = true;
    break;
  case 'f':
    if (tag == "fieldset" || tag == "form")
      pending_space = true;
    break;
  case 'h':
    // hr, and h1, ..., h6
    if (tag.length() == 2 && strchr("r123456", tag[1]))
      pending_space = true;
    break;
  case 'i':
    if (tag == "iframe" || tag == "img" || tag == "isindex" || tag == "input")
      pending_space = true;
    break;
  case 'k':
    if (tag == "keygen")
      pending_space = true;
    break;
  case 'l':
    if (tag == "legend" || tag == "li" || tag == "listing")
      pending_space = true;
    break;
  case 'm':
    if (tag == "meta") {
      string content;
      if (get_parameter("content", content)) {
        string name;
        if (get_parameter("name", name)) {
          lowercase_string(name);
          if (name == "description") {
            if (sample.empty()) {
              swap(sample, content);
              // convert_to_utf8(sample, charset);
              decode_entities(sample);
            }
          } else if (name == "keywords") {
            if (!keywords.empty())
              keywords += ' ';
            // convert_to_utf8(content, charset);
            decode_entities(content);
            keywords += content;
          } else if (name == "robots") {
            decode_entities(content);
            lowercase_string(content);
            if (content.find("none") != string::npos ||
                content.find("noindex") != string::npos) {
              indexing_allowed = false;
              throw true;
            }
          } else if (name == "geo.position") {
            auto sep_pos = content.find(";");
            if (sep_pos != string::npos) {
              double parsed_latitude = 0;
              double parsed_longitude = 0;
              if (parse_coordinate(content.substr(0, sep_pos), 90, parsed_latitude) &&
                  parse_coordinate(content.substr(sep_pos + 1), 180, parsed_longitude)) {
                latitude = parsed_latitude;
                longitude = parsed_longitude;
                has_geoPosition = true;
              }
            }
          }
          break;
        }
        // If the current charset came from a meta tag, don't
        // force reparsing again!
        if (charset_from_meta)
          break;
        string hdr;
        if (get_parameter("http-equiv", hdr)) {
          lowercase_string(hdr);
          if (hdr == "content-type") {
            lowercase_string(content);
            size_t start = content.find("charset=");
            if (start == string::npos)
              break;
            start += 8;
            if (start == content.size())
              break;
            size_t end = start;
            if (content[start] != '"') {
              while (end < content.size()) {
                unsigned char ch = content[end];
                if (ch <= 32 || ch >= 127 || strchr(";()<>@,:\\\"/[]?={}", ch))
                  break;
                ++end;
              }
            } else {
              ++start;
              ++end;
              while (end < content.size()) {
                unsigned char ch = content[end];
                if (ch == '"')
                  break;
                if (ch == '\\')
                  content.erase(end, 1);
                ++end;
              }
            }
            string newcharset(content, start, end - start);
            if (charset != newcharset) {
              throw newcharset;
            }
          }
        }
        break;
      }
      if (charset_from_meta)
        break;
      string newcharset;
      if (get_parameter("charset", newcharset)) {
        // HTML5 added: <meta charset="...">
        lowercase_string(newcharset);
        if (charset != newcharset) {
          throw newcharset;
        }
      }
      break;
    }
    if (tag == "marquee" || tag == "menu" || tag == "multicol")
      pending_space = true;
    break;
  case 'o':
    if (tag == "ol" || tag == "option")
      pending_space = true;
    break;
  case 'p':
    if (tag == "p" || tag == "pre" || tag == "plaintext")
      pending_space = true;
    break;
  case 'q':
    if (tag == "q")
      pending_space = true;
    break;
  case 's':
    if (tag == "style") {
      in_style_tag = true;
      break;
    }
    if (tag == "script") {
      in_script_tag = true;
      break;
    }
    if (tag == "select")
      pending_space = true;
    break;
  case 't':
    if (tag == "table" || tag == "td" || tag == "textarea" || tag == "th")
      pending_space = true;
    break;
  case 'u':
    if (tag == "ul")
      pending_space = true;
    break;
  case 'x':
    if (tag == "xmp")
      pending_space = true;
    break;
  }
}

void zim::MyHtmlParser::closing_tag(const string &tag) {
  if (tag.empty())
    return;
  switch (tag[0]) {
  case 'a':
    if (tag == "address")
      pending_space = true;
    break;
  case 'b':
    if (tag == "body") {
      throw true;
    }
    if (tag == "blockquote" || tag == "br")
      pending_space = true;
    break;
  case 'c':
    if (tag == "center")
      pending_space = true;
    break;
  case 'd':
    if (tag == "dd" || tag == "dir" || tag == "div" || tag == "dl" ||
        tag == "dt")
      pending_space = true;
    break;
  case 'f':
    if (tag == "fieldset" || tag == "form")
      pending_space = true;
    break;
  case 'h':
    // hr, and h1, ..., h6
    if (tag.length() == 2 && strchr("r123456", tag[1]))
      pending_space = true;
    break;
  case 'i':
    if (tag == "iframe")
      pending_space = true;
    break;
  case 'l':
    if (tag == "legend" || tag == "li" || tag == "listing")
      pending_space = true;
    break;
  case 'm':
    if (tag == "marquee" || tag == "menu")
      pending_space = true;
    break;
  case 'o':
    if (tag == "ol" || tag == "option")
      pending_space = true;
    break;
  case 'p':
    if (tag == "p" || tag == "pre")
      pending_space = true;
    break;
  case 'q':
    if (tag == "q")
      pending_space = true;
    break;
  case 's':
    if (tag == "style") {
      in_style_tag = false;
      break;
    }
    if (tag == "script") {
      in_script_tag = false;
      break;
    }
    if (tag == "select")
      pending_space = true;
    break;
  case 't':
    if (tag == "title") {
      if (title.empty())
        swap(title, dump);
      break;
    }
    if (tag == "table" || tag == "td" || tag == "textarea" || tag == "th")
      pending_space = true;
    break;
  case 'u':
    if (tag == "ul")
      pending_space = true;
    break;
  case 'x':
    if (tag == "xmp")
      pending_space = true;
    break;
  }
}
