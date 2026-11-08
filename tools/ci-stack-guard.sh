#!/usr/bin/env bash
# Fails a stacked pull request (one whose base isn't main) that changes
# anything besides code. It lists what code is rather than what it isn't, so
# a new kind of config file is blocked without updating this script. Docs,
# agent rules, CI, Nix and tooling changes go in their own pull request off
# main, never in the middle of a stack.
#
# Usage: tools/ci-stack-guard.sh <base branch> [<main> <head>]
# Reads the changed paths on stdin, NUL-separated (`git diff --name-only -z`),
# so unusual file names arrive unquoted. Given main and the pushed head, a
# path that is the same in both is let through: the branch got it by merging
# main, it isn't the branch's own change.
set -euo pipefail

base=${1:?usage: tools/ci-stack-guard.sh <base branch> [<main> <head>]}
main=${2:-}
head=${3:-}
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
blocked=""
while IFS= read -r -d '' path; do
    if [ -n "$main" ] && [ "$(git rev-parse -q --verify "$main:$path" || true)" = "$(git rev-parse -q --verify "$head:$path" || true)" ]; then
        continue
    fi
    blocked+="$path "
done < <(grep -zvE "$code" "$paths" || true; grep -zE "$code" "$paths" | grep -zE "$not_code" || true)
if [ -n "$blocked" ]; then
    echo "A stacked pull request (base $base) may change code only; move these to a pull request off main: $blocked" >&2
    exit 1
fi
