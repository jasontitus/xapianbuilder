/* htmlparse.h: simple HTML parser for omega indexer
 *
 * Copyright 1999,2000,2001 BrightStation PLC
 * Copyright 2002,2006,2008 Olly Betts
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
 * 2026-04-27: imported from libzim; removed libzim/config includes and the
 * LIBZIM_PRIVATE_API annotation for the standalone build.
 * 2026-09-18: added script/style raw-text state and a transparent entity-map
 * comparator for allocation-free lookup.
 * 2026-09-19: added this modification notice. See UPSTREAM.md for provenance.
 */

#ifndef OMEGA_INCLUDED_HTMLPARSE_H
#define OMEGA_INCLUDED_HTMLPARSE_H

// vendored: removed zim.h include
// vendored: removed config.h include (libzim meson-generated, not needed here)

#include <string>
#include <map>

using std::string;
using std::map;

namespace zim {

class HtmlParser {
	map<string, string> parameters;
    protected:
	void decode_entities(string &s);
	string raw_text_tag;
	string charset;
	static map<string, unsigned int, std::less<>> named_ents;

	bool get_parameter(const string & param, string & value);
    public:
	virtual void process_text(const string &/*text*/) { }
	virtual void opening_tag(const string &/*tag*/) { }
	virtual void closing_tag(const string &/*tag*/) { }
	virtual void parse_html(const string &text);
	HtmlParser();
	virtual ~HtmlParser() { }
};

};

#endif // OMEGA_INCLUDED_HTMLPARSE_H
