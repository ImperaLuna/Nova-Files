#!/usr/bin/env bash
# Fails a stacked pull request (one whose base isn't main) that changes
# anything besides code. Docs, agent rules, CI and tooling changes go in
# their own pull request off main, never in the middle of a stack.
#
# Usage: tools/ci-stack-guard.sh <base branch>
# Reads the changed paths, one per line, on stdin.
set -euo pipefail

base=${1:?usage: tools/ci-stack-guard.sh <base branch>}
if [ "$base" = main ]; then
    exit 0
fi

blocked=$(grep -E '(\.md$|^\.github/|^\.cargo/|^justfile$|(^|/)deny\.toml$|(^|/)clippy\.toml$)' || true)
if [ -n "$blocked" ]; then
    echo "A stacked pull request (base $base) may change code only; move these to a pull request off main: $(paste -sd ' ' <<<"$blocked")" >&2
    exit 1
fi
