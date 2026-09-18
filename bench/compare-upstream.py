#!/usr/bin/env python3
"""Rebuild the committed corpus with installed upstream libzim and xapianbuilder.

Requires Python >= 3.9, a C++17 compiler, pkg-config, libzim (with Xapian
indexing), Xapian development packages, and a built xapianbuilder. No zimru,
external ZIM archive, Python packages, network access, or private libzim API.

Exit: 0 exact semantic match; 1 mismatch; 2 setup/tool/input error;
3 inconclusive (no nonempty baseline). Every run retains a unique workspace.
"""

import argparse
import difflib
import hashlib
import json
import os
from pathlib import Path
import platform
import runpy
import shlex
import shutil
import subprocess
import sys
import tempfile


HERE = Path(__file__).resolve().parent
ROOT = HERE.parent


class Failure(Exception):
    pass


def digest(path):
    with path.open("rb") as stream:
        value = hashlib.sha256()
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            value.update(chunk)
        return value.hexdigest()


def executable(value):
    found = shutil.which(value)
    if not found:
        raise Failure("missing or non-executable tool: " + value)
    return str(Path(found).resolve())


def fixture_records():
    generator = runpy.run_path(str(HERE / "generate-fixture.py"))
    fixture = (HERE / "fixture.jsonl").read_bytes()
    if fixture != generator["fixture_bytes"]():
        raise Failure("committed fixture differs from generate-fixture.py; regenerate and review it")
    records = [json.loads(line) for line in fixture.decode("utf-8").splitlines()]
    if not records:
        raise Failure("fixture is empty")
    return records


def prepare_fixture(records, workspace):
    manifest = []
    fulltext = []
    for index, record in enumerate(records):
        target = record.get("target_path", "")
        body_name = "body-{:03d}.html".format(index) if not target else ""
        fields = [record["path"], record["title"], record.get("mimetype", ""), body_name, target]
        if any("\t" in field or "\n" in field or "\r" in field for field in fields):
            raise Failure("fixture field cannot be encoded in the helper manifest")
        manifest.append("\t".join(fields))
        if not target:
            (workspace / body_name).write_text(record["body"], encoding="utf-8")
            fulltext.append(record)
    (workspace / "manifest.tsv").write_text("\n".join(manifest) + "\n", encoding="utf-8")
    shutil.copyfile(HERE / "fixture.jsonl", workspace / "title.jsonl")
    (workspace / "fulltext.jsonl").write_text(
        "".join(json.dumps(record, ensure_ascii=False) + "\n" for record in fulltext), encoding="utf-8")


def source_hashes():
    paths = [ROOT / name for name in ("Cargo.toml", "Cargo.lock", "build.rs")]
    for directory in ("src", "cpp", "data"):
        paths.extend(path for path in (ROOT / directory).rglob("*") if path.is_file())
    paths.extend(path for path in HERE.iterdir() if path.is_file())
    return {str(path.relative_to(ROOT)): digest(path) for path in sorted(paths)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--builder", default=os.environ.get("XAPIANBUILDER", str(ROOT / "target/release/xapianbuilder")))
    parser.add_argument("--out", type=Path, help="existing parent directory for a new retained workspace")
    parser.add_argument("--source-revision", help="source revision for a checkout without .git (otherwise read HEAD)")
    args = parser.parse_args()
    report = {"status": "error", "commands": [], "comparison": {},
              "python": sys.version, "platform": platform.platform()}
    workspace = None
    try:
        builder = executable(args.builder)
        compiler = executable(os.environ.get("CXX", "c++"))
        pkg_config = executable(os.environ.get("PKG_CONFIG", "pkg-config"))
        records = fixture_records()
        workspace = Path(tempfile.mkdtemp(prefix="upstream-comparison-", dir=args.out)).resolve()
        print("Artifacts: " + str(workspace), flush=True)

        def run(command, label, *, strict_stderr=False):
            number = len(report["commands"])
            log = workspace / ("{:02d}-{}".format(number, label))
            invocation = {"argv": [str(arg) for arg in command], "log": str(log.name)}
            report["commands"].append(invocation)
            result = subprocess.run(invocation["argv"], cwd=ROOT, capture_output=True,
                                    text=True, encoding="utf-8", errors="strict", timeout=300)
            invocation["returncode"] = result.returncode
            log.with_suffix(".stdout").write_text(result.stdout, encoding="utf-8")
            log.with_suffix(".stderr").write_text(result.stderr, encoding="utf-8")
            if result.returncode or (strict_stderr and result.stderr.strip()):
                raise Failure("{} failed (status {}); see {}.*".format(label, result.returncode, log))
            return result.stdout

        report["source_revision"] = args.source_revision or run(
            [executable("git"), "rev-parse", "HEAD"], "source-revision").strip()
        if not report["source_revision"]:
            raise Failure("cannot establish source revision")
        if not args.source_revision:
            report["git_status"] = run([executable("git"), "status", "--porcelain=v1"], "source-status")
        report["source_sha256"] = source_hashes()
        report["libzim_source_revision"] = os.environ.get(
            "LIBZIM_REFERENCE_REVISION", "unknown (installed package)")
        report["fixture_sha256"] = digest(HERE / "fixture.jsonl")
        report["builder_sha256"] = digest(Path(builder))
        report["builder_version"] = run([builder, "version"], "builder-version", strict_stderr=True).strip()
        report["compiler_version"] = run([compiler, "--version"], "compiler-version").strip()
        report["pkg_config_version"] = run([pkg_config, "--version"], "pkg-config-version").strip()
        report["pkg_config_path"] = os.environ.get("PKG_CONFIG_PATH", "")
        report["library_package_versions"] = {
            name: run([pkg_config, "--modversion", name], "version-" + name, strict_stderr=True).strip()
            for name in ("libzim", "xapian-core")}
        flags = shlex.split(run([pkg_config, "--cflags", "--libs", "libzim", "xapian-core"],
                                "native-flags", strict_stderr=True))
        helper = workspace / "upstream-helper"
        compile_command = [compiler, "-std=c++17", *shlex.split(os.environ.get("CXXFLAGS", "")),
                           str(HERE / "upstream-helper.cc"), "-o", str(helper), *flags,
                           *shlex.split(os.environ.get("LDFLAGS", ""))]
        run(compile_command, "compile-helper")
        report["library_runtime_versions"] = run([helper, "versions"], "runtime-versions", strict_stderr=True)
        prepare_fixture(records, workspace)
        run([helper, "create", workspace / "manifest.tsv", workspace / "upstream"], "create-upstream")
        mismatched = False
        inconclusive = False
        for mode in ("title", "fulltext"):
            candidate_db = workspace / (mode + ".xapian")
            run([builder, mode, "--input", workspace / (mode + ".jsonl"),
                 "--output", candidate_db, "--language", "eng", "--jobs", "1", "--quiet"],
                "build-" + mode)
            baseline = run([helper, "snapshot", workspace / "upstream" / (mode + ".xapian")],
                           "snapshot-upstream-" + mode, strict_stderr=True)
            candidate = run([helper, "snapshot", candidate_db], "snapshot-builder-" + mode,
                            strict_stderr=True)
            baseline_lines = baseline.splitlines(keepends=True)
            candidate_lines = candidate.splitlines(keepends=True)
            documents = sum(line.startswith("DOC ") for line in baseline_lines)
            terms = sum(line.startswith("TERM ") for line in baseline_lines)
            values = sum(line.startswith("VALUE ") for line in baseline_lines)
            if not documents or not terms or not values:
                inconclusive = True
            equal = baseline == candidate
            mismatched |= not equal
            report["comparison"][mode] = {"equal": equal, "baseline_documents": documents,
                                           "baseline_terms": terms, "baseline_values": values}
            (workspace / (mode + ".diff")).write_text("".join(difflib.unified_diff(
                baseline_lines, candidate_lines, fromfile="upstream/" + mode, tofile="builder/" + mode)),
                encoding="utf-8")
            print("{}: {} upstream documents, {} terms, {} values, {}".format(
                mode, documents, terms, values,
                "MATCH" if equal and documents and terms and values else "NOT VERIFIED"))
        status = 3 if inconclusive else (1 if mismatched else 0)
        report["status"] = {0: "semantic-match", 1: "mismatch", 3: "inconclusive"}[status]
        report["exit_status"] = status
        print("Result: " + report["status"] + "; details: " + str(workspace / "report.json"))
        return status
    except (Failure, OSError, ValueError, UnicodeError, subprocess.TimeoutExpired) as error:
        report["error"] = str(error)
        report["exit_status"] = 2
        print("ERROR: " + str(error), file=sys.stderr)
        return 2
    finally:
        if workspace is not None:
            (workspace / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n",
                                                    encoding="utf-8")


if __name__ == "__main__":
    sys.exit(main())
