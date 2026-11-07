#!/usr/bin/env bash
# Fails a stacked pull request (one whose base isn't main) that changes
# anything besides code. It lists what code is rather than what it isn't, so
# a new kind of config file is blocked without updating this script. Docs,
# agent rules, CI, Nix and tooling changes go in their own pull request off
# main, never in the middle of a stack.
#
# Usage: tools/ci-stack-guard.sh <base branch>
# Reads the changed paths on stdin, NUL-separated (`git diff --name-only -z`),
# so unusual file names arrive unquoted.
set -euo pipefail

base=${1:?usage: tools/ci-stack-guard.sh <base branch>}
if [ "$base" = main ]; then
    exit 0
fi

# Code is the crates, the workspace manifest and lock file, and data/ (the
# app's own assets). Anything else is blocked, as is Markdown or a lint or
# format config inside a crate.
code='^(crates/|data/|Cargo\.toml$|Cargo\.lock$)'
not_code='(\.md$|(^|/)\.?(clippy|rustfmt|deny)\.toml$)'
paths=$(mktemp)
trap 'rm -f "$paths"' EXIT
cat >"$paths"
blocked=$( (grep -zvE "$code" "$paths" || true; grep -zE "$code" "$paths" | grep -zE "$not_code" || true) | tr '\0' ' ')
if [ -n "$blocked" ]; then
    echo "A stacked pull request (base $base) may change code only; move these to a pull request off main: $blocked" >&2
    exit 1
fi
