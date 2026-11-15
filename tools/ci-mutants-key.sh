#!/usr/bin/env bash
# Prints `key=<cache key>` for the mutation tests of a pull request, made
# from the Rust lines the pull request itself changes. CI saves a marker
# under it once every shard passes, and a later run with the same key skips
# the tests. So after a lower PR of a stack merges and this one moves onto
# main, or main is merged in, unchanged code of its own passes in seconds.
#
# The diff runs from the base branch (HEAD^1 of GitHub's merge commit) to
# the merge commit, with no context lines and without the hunks' line
# numbers or blob ids, so moved lines elsewhere in main don't change it.
# Bump VERSION to make every pull request run its mutants again.
set -euo pipefail

VERSION=1
base=${1:-HEAD^1}

hash=$(
    git diff -U0 --no-color --no-ext-diff --no-renames "$base" HEAD -- '*.rs' |
        sed -e '/^index /d' -e 's/^@@ .*/@@/' |
        sha256sum | cut -d' ' -f1
)
echo "key=mutants-passed-v$VERSION-$hash"
