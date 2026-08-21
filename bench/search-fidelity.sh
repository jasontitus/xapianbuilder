#!/usr/bin/env bash
#
# Search fidelity check: does an index built by xapianbuilder answer the same
# queries, with the same ranking, as the libzim-built index in the source
# archive?
#
# For each source ZIM we rebuild it with zimru's `zimrecreate` (which spawns
# xapianbuilder for X/fulltext/xapian and X/title/xapian), then put the same
# queries to both archives through upstream `zimsearch` — a libzim/Xapian
# binary that knows nothing about how either index was produced — and compare
# the result lists.
#
# Queries are drawn from the archive itself (article titles sampled across
# title order), so they exercise the language's own script: CJK n-grams for
# zh/ja/ko/yue, abugida clusters for hi/ta, RTL for ar/he, no-space
# segmentation for th.
#
# Usage: bench/search-fidelity.sh <zim> [<zim>...]
#   env: UPSTREAM_DIR ZIMRU_RECREATE ZIMRU_DUMP XAPIANBUILDER QUERIES OUT
set -uo pipefail

UPSTREAM_DIR="${UPSTREAM_DIR:-/opt/zim-tools-upstream/zim-tools_linux-x86_64-3.8.0}"
UP_SEARCH="$UPSTREAM_DIR/zimsearch"
ZIMRU_RECREATE="${ZIMRU_RECREATE:-/home/user/zimru/target/release/zimrecreate}"
ZIMRU_DUMP="${ZIMRU_DUMP:-/home/user/zimru/target/release/zimdump}"
export XAPIANBUILDER="${XAPIANBUILDER:-/home/user/xapianbuilder/target/release/xapianbuilder}"
QUERIES="${QUERIES:-25}"
OUT="${OUT:-/tmp/search-fidelity}"
mkdir -p "$OUT"

# zimsearch prints "article <id>" then "score <n>\t:\t<title>" per hit. Only
# the ranked title list matters for comparison.
hits() { $UP_SEARCH "$1" "$2" 2>/dev/null | sed -n 's/^score [0-9]*\t:\t//p'; }

printf "%-40s %8s %9s %9s %9s  %s\n" FILE QUERIES IDENTICAL TOP1 NONEMPTY NOTE
printf -- "%s\n" "$(printf '%.0s-' {1..100})"

for src in "$@"; do
    [[ -f "$src" ]] || { echo "skip (missing): $src" >&2; continue; }
    name=$(basename "$src" .zim)
    out="$OUT/$name.rebuilt.zim"
    rm -f "$out"
    if ! $ZIMRU_RECREATE "$src" "$out" --compression zstd >/dev/null 2>&1; then
        printf "%-40s %8s %9s %9s %9s  %s\n" "$name" - - - - "recreate FAILED"
        continue
    fi
    if [[ $($ZIMRU_DUMP list --ns=X "$out" 2>/dev/null | grep -c 'xapian$') != 2 ]]; then
        printf "%-40s %8s %9s %9s %9s  %s\n" "$name" - - - - "no index built"
        rm -f "$out"; continue
    fi

    # Sample titles evenly across the archive rather than taking a prefix, so
    # the queries aren't all from one alphabetical neighbourhood.
    mapfile -t qs < <($ZIMRU_DUMP list "$src" 2>/dev/null \
        | awk 'length($0) >= 3 && $0 !~ /[.\/]/' \
        | awk -v n="$QUERIES" '{a[NR]=$0} END{ if (NR==0) exit; step=int(NR/n); if (step<1) step=1; for (i=1; i<=NR && c<n; i+=step) {print a[i]; c++} }')

    total=0; identical=0; top1=0; nonempty=0
    for q in "${qs[@]}"; do
        [[ -n "$q" ]] || continue
        total=$((total+1))
        a=$(hits "$src" "$q"); b=$(hits "$out" "$q")
        [[ -n "$a" ]] && nonempty=$((nonempty+1))
        [[ "$a" == "$b" ]] && identical=$((identical+1))
        [[ "$(head -1 <<<"$a")" == "$(head -1 <<<"$b")" ]] && top1=$((top1+1))
    done
    note=""
    [[ "$identical" == "$total" ]] || note="see $OUT/$name.diff"
    if [[ -n "$note" ]]; then
        : > "$OUT/$name.diff"
        for q in "${qs[@]}"; do
            [[ -n "$q" ]] || continue
            a=$(hits "$src" "$q"); b=$(hits "$out" "$q")
            [[ "$a" == "$b" ]] && continue
            { echo "### query: $q"; diff <(echo "$a") <(echo "$b"); } >> "$OUT/$name.diff"
        done
    fi
    printf "%-40s %8d %9d %9d %9d  %s\n" "$name" "$total" "$identical" "$top1" "$nonempty" "$note"
    [[ -n "${KEEP:-}" ]] || rm -f "$out"
done
