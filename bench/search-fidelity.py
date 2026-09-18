#!/usr/bin/env python3
"""Compare ranked zimsearch titles; optional zimru integration, not the core recipe.

Exit codes: 0 verified match with a nonempty baseline for EVERY archive;
1 mismatch; 2 dependency/input/tool/protocol failure; 3 inconclusive baseline.
All artifacts live in a new, retained directory under OUT (default: system temp).
No supplied path is removed or overwritten. Tool variables name executables,
not shell commands. Unsupported search output fails closed.
"""

import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile


class Failure(Exception):
    pass


def executable(value):
    found = shutil.which(value)
    if not found:
        raise Failure("missing or non-executable tool: " + value)
    return str(Path(found).resolve())


def run(command, log, *, allow_stderr=False):
    try:
        result = subprocess.run(command, capture_output=True, text=True,
                                encoding="utf-8", errors="strict", timeout=600)
    except (OSError, UnicodeError, subprocess.TimeoutExpired) as error:
        raise Failure("cannot execute {!r}: {}".format(command, error)) from error
    log.with_suffix(".stdout").write_text(result.stdout, encoding="utf-8")
    log.with_suffix(".stderr").write_text(result.stderr, encoding="utf-8")
    log.with_suffix(".command.json").write_text(
        json.dumps({"argv": command, "returncode": result.returncode}, indent=2) + "\n",
        encoding="utf-8")
    if result.returncode:
        raise Failure("tool exited {}: {!r}; logs: {}.*".format(
            result.returncode, command, log))
    # zimsearch historically catches exceptions without a nonzero exit status.
    # Treat even warnings as inconclusive execution, never as an empty result.
    if result.stderr.strip() and not allow_stderr:
        raise Failure("tool reported diagnostics: {!r}; log: {}.stderr".format(command, log))
    return result.stdout


def hits(text):
    """Accept only complete article/score pairs from upstream zimsearch."""
    lines = [line for line in text.splitlines() if line.strip()]
    if len(lines) % 2:
        raise Failure("unrecognized or incomplete zimsearch output")
    titles = []
    for i in range(0, len(lines), 2):
        if not re.fullmatch(r"article [0-9]+", lines[i]):
            raise Failure("unrecognized zimsearch article line: " + repr(lines[i]))
        match = re.fullmatch(r"score [0-9]+\t:\t(.+)", lines[i + 1])
        if not match:
            raise Failure("unrecognized zimsearch score line: " + repr(lines[i + 1]))
        titles.append(match.group(1))
    return titles


def compare(source, index, workspace, tools, query_limit):
    case = workspace / ("archive-{:03d}".format(index))
    case.mkdir()
    rebuilt = case / "rebuilt.zim"
    run([tools["recreate"], str(source), str(rebuilt), "--compression", "zstd"],
        case / "recreate", allow_stderr=True)
    if not rebuilt.is_file() or not rebuilt.stat().st_size:
        raise Failure("recreate produced no nonempty archive: " + str(rebuilt))
    indexes = run([tools["dump"], "list", "--ns=X", str(rebuilt)], case / "indexes")
    paths = {line.strip().removeprefix("X/") for line in indexes.splitlines()}
    if not {"fulltext/xapian", "title/xapian"}.issubset(paths):
        raise Failure("rebuilt archive does not list both Xapian indexes")
    listing = run([tools["dump"], "list", str(source)], case / "queries")
    candidates = list(dict.fromkeys(line for line in listing.splitlines()
                                   if len(line) >= 3 and "." not in line and "/" not in line))
    count = min(query_limit, len(candidates))
    queries = [candidates[i * len(candidates) // count] for i in range(count)]
    report = {"source": str(source), "queries": [], "nonempty_baselines": 0}
    mismatched = False
    for number, query in enumerate(queries):
        baseline = hits(run([tools["search"], str(source), query],
                            case / ("query-{:03d}-baseline".format(number))))
        candidate = hits(run([tools["search"], str(rebuilt), query],
                             case / ("query-{:03d}-candidate".format(number))))
        report["nonempty_baselines"] += bool(baseline)
        mismatched |= baseline != candidate
        report["queries"].append({"query": query, "baseline": baseline,
                                  "candidate": candidate, "equal": baseline == candidate})
    status = 1 if mismatched else (0 if report["nonempty_baselines"] else 3)
    report["exit_status"] = status
    (case / "comparison.json").write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n",
                                           encoding="utf-8")
    print("{}: {} queries, {} nonempty baselines, {}".format(
        source, count, report["nonempty_baselines"],
        {0: "MATCH (ranked titles only)", 1: "MISMATCH", 3: "INCONCLUSIVE"}[status]))
    return status


def main():
    if not sys.argv[1:]:
        print("usage: bench/search-fidelity.sh archive.zim [archive.zim ...]", file=sys.stderr)
        return 2
    try:
        query_limit = int(os.environ.get("QUERIES", "25"))
        if query_limit <= 0:
            raise Failure("QUERIES must be a positive integer")
        upstream = os.environ.get("UPSTREAM_DIR")
        tools = {
            "search": executable(os.environ.get("UP_SEARCH") or
                                 (str(Path(upstream) / "zimsearch") if upstream else "zimsearch")),
            "recreate": executable(os.environ.get("ZIMRU_RECREATE", "zimrecreate")),
            "dump": executable(os.environ.get("ZIMRU_DUMP", "zimdump")),
            "builder": executable(os.environ.get("XAPIANBUILDER", "xapianbuilder")),
        }
        os.environ["XAPIANBUILDER"] = tools["builder"]
        sources = [Path(value).resolve(strict=True) for value in sys.argv[1:]]
        if any(not path.is_file() for path in sources):
            raise Failure("every input must be a regular file")
        workspace = Path(tempfile.mkdtemp(prefix="search-fidelity-", dir=os.environ.get("OUT"))).resolve()
        print("Artifacts: " + str(workspace), flush=True)
        statuses = []
        for index, source in enumerate(sources):
            try:
                statuses.append(compare(source, index, workspace, tools, query_limit))
            except (Failure, OSError, UnicodeError) as error:
                print("ERROR {}: {}".format(source, error), file=sys.stderr)
                statuses.append(2)
        return next((status for status in (2, 1, 3) if status in statuses), 0)
    except (Failure, OSError, ValueError) as error:
        print("ERROR: " + str(error), file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
