#!/usr/bin/env bash
# Fails when a Rust file under crates/ is longer than MAX_LINES.
#
# Aim for modules under about 500 lines; past MAX_LINES a file must be split
# into smaller modules that each do one job.
set -euo pipefail

readonly MAX_LINES=800

status=0
while IFS= read -r -d '' file; do
    lines=$(wc -l <"$file")
    if ((lines > MAX_LINES)); then
        echo "$file: $lines lines (limit $MAX_LINES). Split it into smaller modules."
        status=1
    fi
done < <(find crates -name '*.rs' -print0)
exit "$status"
