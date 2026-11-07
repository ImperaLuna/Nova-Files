#!/usr/bin/env bash
# Fails a stacked pull request (one whose base isn't main) that changes
# anything besides code. Docs, agent rules, CI, Nix and tooling changes go in
# their own pull request off main, never in the middle of a stack.
#
# Usage: tools/ci-stack-guard.sh <base branch>
# Reads the changed paths on stdin, NUL-separated (`git diff --name-only -z`),
# so unusual file names arrive unquoted.
set -euo pipefail

base=${1:?usage: tools/ci-stack-guard.sh <base branch>}
if [ "$base" = main ]; then
    exit 0
fi

blocked=$(grep -zE '(\.md$|^nix/|^flake\.|^\.github/|^tools/|^\.cargo/|^justfile$|(^|/)deny\.toml$|(^|/)clippy\.toml$)' | tr '\0' ' ' || true)
if [ -n "$blocked" ]; then
    echo "A stacked pull request (base $base) may change code only; move these to a pull request off main: $blocked" >&2
    exit 1
fi
