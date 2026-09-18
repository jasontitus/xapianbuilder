#!/usr/bin/env python3
"""Emit the original, redistributable comparison corpus (UTF-8 JSONL).

All prose is invented for this project; no downloaded archive content is used.
The fixture and this generator are distributed under the repository's GPL-3.0
license. Regenerate with: python3 bench/generate-fixture.py > /tmp/fixture.jsonl
The comparison recipe refuses a committed fixture that differs from this output.
"""

import json
import sys


def fixture_bytes():
    pages = [
        ("orchard", "Café Orchard", '<meta name="keywords" content="apples harvest"><meta name="geo.position" content="48.85;2.35">',
         "<p>The orchard grows apples and pears. Apples ripen beside the café.</p>"),
        ("harbor", "Harbor Lantern", "",
         "<p>A lantern guides boats into the harbor. Orchard apples arrive by boat.</p>"),
        ("garden", "中文 Garden", "",
         "<p>中文花园 grows green tea. Garden paths connect the harbor and the orchard.</p>"),
        ("entities", "Résumé & Notes", '<meta name="keywords" content="résumé research">',
         "<p>A résumé records research &amp; discoveries. A&#160;small note mentions naïve ideas.</p>"
         "<script>hidden_script_token</script><style>hidden_style_token</style><!-- hidden_comment_token -->"),
        ("symbols", "???", "", "<p>Symbols describe a question. The answer concerns lanterns.</p>"),
        ("noindex", "Private Notebook", '<meta name="robots" content="noindex">',
         "<p>This notebook must not enter the fulltext index.</p>"),
        ("empty", "Empty Page", "", "<script>only_script_token</script>"),
    ]
    records = []
    for path, title, metadata, content in pages:
        records.append({"path": path, "title": title, "mimetype": "text/html",
                        "body": '<!doctype html><html><head><meta charset="utf-8">' + metadata
                                + "</head><body>" + content + "</body></html>"})
    records.append({"path": "apple-alias", "title": "Apple Orchard", "target_path": "orchard"})
    return ("\n".join(json.dumps(record, ensure_ascii=False, separators=(",", ":"))
                       for record in records) + "\n").encode("utf-8")


if __name__ == "__main__":
    sys.stdout.buffer.write(fixture_bytes())
