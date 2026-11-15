#!/usr/bin/env bash
# Prints `key=<cache key>` for the mutation tests of a pull request, made
# from the Rust lines the pull request itself changes. CI saves a marker
# under it once every shard passes, and a later run with the same key skips
# the tests. So after a lower PR of a stack merges and this one moves onto
# main, or main is merged in, unchanged code of its own passes in seconds.
#
# The diff runs from the base branch (HEAD^1 of GitHub's merge commit) to
# the merge commit, with no context lines and without the hunks' line
# numbers or blob ids, so lines main moves around don't change it. Each
# hunk keeps the function it sits in, and .cargo/mutants.toml goes in too.
# The rest of the base is left out on purpose: a pass stays a pass when
# only main changes. Bump VERSION to make every pull request run again.
set -euo pipefail

cd "$(dirname "$0")/.."
VERSION=1
base=${1:-HEAD^1}

hash=$(
    {
        git diff -U0 --no-color --no-ext-diff --no-renames "$base" HEAD -- '*.rs' |
            sed -e '/^index /d' -e 's/^@@ [^@]* @@/@@/'
        cat .cargo/mutants.toml
    } | sha256sum | cut -d' ' -f1
)
echo "key=mutants-passed-v$VERSION-$hash"
