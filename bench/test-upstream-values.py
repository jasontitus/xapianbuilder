#!/usr/bin/env python3
"""Prove the real upstream comparison detects value-only changes without termlists.

Requires the same native dependencies as compare-upstream.py. The candidate is
built by the real xapianbuilder; only a displayed title is uppercased, leaving
normalized title terms, positions and weights unchanged.
"""

import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--builder", required=True, type=Path)
    args = parser.parse_args()
    builder = args.builder.resolve()
    comparator = Path(__file__).resolve().with_name("compare-upstream.py")
    with tempfile.TemporaryDirectory(prefix="value-comparison-regression-") as directory:
        workspace = Path(directory)
        wrapper = workspace / "mutating-builder"
        wrapper.write_text(
            "#!/usr/bin/env python3\n"
            "import json, os, pathlib, sys\n"
            "args = sys.argv[1:]\n"
            "if args[0] == 'title':\n"
            "    source = pathlib.Path(args[args.index('--input') + 1])\n"
            "    docs = [json.loads(line) for line in source.read_text(encoding='utf-8').splitlines()]\n"
            "    docs[0]['title'] = docs[0]['title'].upper()\n"
            "    source.write_text(''.join(json.dumps(doc) + '\\n' for doc in docs), encoding='utf-8')\n"
            "os.execv(os.environ['REAL_BUILDER'], [os.environ['REAL_BUILDER'], *args])\n",
            encoding="utf-8",
        )
        wrapper.chmod(0o755)
        result = subprocess.run(
            [sys.executable, str(comparator), "--builder", str(wrapper), "--out", "."],
            cwd=workspace,
            env=dict(os.environ, REAL_BUILDER=str(builder)),
            capture_output=True, text=True, timeout=300,
        )
        if result.returncode != 1:
            raise RuntimeError("value-only mismatch was not rejected:\n" + result.stdout + result.stderr)
        reports = list(workspace.glob("upstream-comparison-*/report.json"))
        if len(reports) != 1:
            raise RuntimeError("comparison did not retain exactly one report")
        report = json.loads(reports[0].read_text(encoding="utf-8"))
        if report["comparison"]["title"]["equal"] or not report["comparison"]["fulltext"]["equal"]:
            raise RuntimeError("mutation did not isolate a title-index mismatch")
        changes = [line for line in reports[0].with_name("title.diff").read_text(encoding="utf-8").splitlines()
                   if line.startswith(("+", "-")) and not line.startswith(("+++", "---"))]
        if len(changes) != 2 or not all(line.startswith(("+VALUE ", "-VALUE ")) for line in changes):
            raise RuntimeError("expected a value-only difference, got: " + repr(changes))
        print("PASS: a value-only title change is rejected with termlists disabled")
    return 0


if __name__ == "__main__":
    sys.exit(main())
