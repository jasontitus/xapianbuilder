#!/bin/sh
# Portable entry point; Python handles subprocess status, parsing and owned output.
# Usage: bench/search-fidelity.sh archive.zim [archive.zim ...]
# See README for prerequisites, environment variables and exit statuses.
set -eu
exec "${PYTHON:-python3}" "$(dirname "$0")/search-fidelity.py" "$@"
