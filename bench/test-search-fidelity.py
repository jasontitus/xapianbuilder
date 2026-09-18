#!/usr/bin/env python3
"""Isolated stub-tool regressions. These do NOT establish real index fidelity."""

import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


RUNNER = Path(__file__).with_name("search-fidelity.sh")
STUB = r'''#!PYTHON
import os
from pathlib import Path
import sys
name = Path(sys.argv[0]).name
mode = os.environ.get("STUB_MODE", "match")
if name == "recreate":
    if mode == "recreate-fails":
        sys.exit(8)
    Path(sys.argv[2]).write_bytes(b"stub archive")
elif name == "dump":
    if mode == "dump-fails":
        sys.exit(9)
    if "--ns=X" in sys.argv:
        print("fulltext/xapian\ntitle/xapian")
    elif mode != "no-queries":
        print("Orchard\nCafé\nHarbor")
elif name == "search":
    if mode == "search-fails":
        sys.exit(7)
    if mode == "candidate-error" and Path(sys.argv[1]).name == "rebuilt.zim":
        print("Could not open rebuilt Xapian database", file=sys.stderr)
        sys.exit(0)
    if mode == "one-empty" and (Path(sys.argv[1]).name == "empty.zim" or
                               Path(sys.argv[1]).parent.name == "archive-001"):
        sys.exit(0)
    if mode == "zero-error":
        print("Could not open Xapian database", file=sys.stderr)
    elif mode == "stdout-error":
        print("ERROR: Cannot open archive")
    elif mode == "partial":
        print("article 1")
    elif mode == "valid-plus-error":
        print("article 1\nscore 100\t:\tOrchard")
        print("index corrupt", file=sys.stderr)
    elif mode not in ("empty", "no-queries"):
        title = "Different" if mode == "mismatch" and Path(sys.argv[1]).name == "rebuilt.zim" else "Orchard"
        print("article 1\nscore 100\t:\t" + title)
'''


class FidelityRegressions(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="fidelity stubs ")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.source = self.root / "source with spaces.zim"
        self.source.write_bytes(b"stub input")
        self.out = self.root / "owned workspace parent"
        self.out.mkdir()
        self.sentinel = self.out / "source with spaces.rebuilt.zim"
        self.sentinel.write_bytes(b"unowned sentinel")
        self.env = os.environ.copy()
        for key in ("UPSTREAM_DIR", "KEEP", "QUERIES", "OUT", "UP_SEARCH",
                    "ZIMRU_RECREATE", "ZIMRU_DUMP", "XAPIANBUILDER", "PYTHON"):
            self.env.pop(key, None)
        self.env.update(OUT=str(self.out), PYTHON=sys.executable)
        for name, variable in (("search", "UP_SEARCH"), ("recreate", "ZIMRU_RECREATE"),
                               ("dump", "ZIMRU_DUMP"), ("builder", "XAPIANBUILDER")):
            tool = self.root / name
            # Match the running interpreter; all fixture tool paths contain spaces.
            tool.write_text(STUB.replace("#!PYTHON", "#!" + sys.executable), encoding="utf-8")
            tool.chmod(0o700)
            self.env[variable] = str(tool)

    def invoke(self, mode="match", args=None):
        self.env["STUB_MODE"] = mode
        result = subprocess.run(["/bin/sh", str(RUNNER), *(args if args is not None else [str(self.source)])],
                                env=self.env, capture_output=True, text=True, timeout=30)
        self.assertEqual(self.sentinel.read_bytes(), b"unowned sentinel")
        return result

    def test_nonempty_match_and_unique_owned_workspaces(self):
        for _ in range(2):
            result = self.invoke()
            self.assertEqual(result.returncode, 0, result.stderr)
        workspaces = list(self.out.glob("search-fidelity-*"))
        self.assertEqual(len(workspaces), 2)
        report = json.loads((workspaces[0] / "archive-000/comparison.json").read_text())
        self.assertEqual(report["nonempty_baselines"], 3)

    def test_missing_search_cannot_pass(self):
        self.env["UP_SEARCH"] = str(self.root / "does not exist")
        result = self.invoke()
        self.assertEqual(result.returncode, 2)
        self.assertNotIn("MATCH", result.stdout)

    def test_tool_errors_and_malformed_search_cannot_pass(self):
        for mode in ("search-fails", "zero-error", "candidate-error", "stdout-error", "partial",
                     "valid-plus-error", "recreate-fails", "dump-fails"):
            with self.subTest(mode=mode):
                result = self.invoke(mode)
                self.assertEqual(result.returncode, 2, result.stdout + result.stderr)
                self.assertNotIn("MATCH", result.stdout)

    def test_empty_baseline_and_no_queries_are_inconclusive(self):
        for mode in ("empty", "no-queries"):
            with self.subTest(mode=mode):
                result = self.invoke(mode)
                self.assertEqual(result.returncode, 3, result.stdout + result.stderr)
                self.assertIn("INCONCLUSIVE", result.stdout)

    def test_one_successful_archive_does_not_mask_an_empty_baseline(self):
        empty = self.root / "empty.zim"
        empty.write_bytes(b"stub input with no indexed content")
        result = self.invoke("one-empty", args=[str(self.source), str(empty)])
        self.assertEqual(result.returncode, 3, result.stdout + result.stderr)
        self.assertIn("MATCH", result.stdout)
        self.assertIn("INCONCLUSIVE", result.stdout)

    def test_ranked_mismatch_is_failure(self):
        result = self.invoke("mismatch")
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("MISMATCH", result.stdout)

    def test_no_inputs_missing_input_and_zero_queries_fail(self):
        self.assertEqual(self.invoke(args=[]).returncode, 2)
        self.assertEqual(self.invoke(args=[str(self.root / "missing.zim")]).returncode, 2)
        self.env["QUERIES"] = "0"
        self.assertEqual(self.invoke().returncode, 2)


if __name__ == "__main__":
    unittest.main()
