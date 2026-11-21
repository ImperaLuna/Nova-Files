#!/usr/bin/env bash
# The pass marker a push needs: `just check` records one for the exact code
# it passed on, and the pre-push hook looks for it, so a push reaches CI
# only after the check passed here.
#
# Usage:
#   local-pass.sh record check   after `nix flake check` passed
#   local-pass.sh verify <sha>   exits 1, saying what to run, when <sha>
#                                lacks a marker
#
# A marker is keyed on the commit's tree. Nothing is recorded while tracked
# files differ from HEAD: the markers name commits. They live in
# target/passed/, which `cargo clean` empties.
set -euo pipefail

root=$(git rev-parse --show-toplevel)
dir="$root/target/passed"

case ${1:-} in
    record)
        if ! git diff --quiet HEAD; then
            echo "Not recording a pass: tracked files differ from HEAD. Commit, then run it again." >&2
            exit 0
        fi
        case ${2:-} in
            check) mkdir -p "$dir" && touch "$dir/check-$(git rev-parse 'HEAD^{tree}')" ;;
            *) echo "usage: local-pass.sh record check" >&2; exit 2 ;;
        esac
        ;;
    verify)
        sha=${2:?usage: local-pass.sh verify <sha>}
        if [ ! -e "$dir/check-$(git rev-parse "$sha^{tree}")" ]; then
            echo "pre-push: ${sha:0:12} has no local pass of just check." >&2
            echo "pre-push: run it on this commit, fix what fails, and push again." >&2
            exit 1
        fi
        ;;
    *)
        echo "usage: local-pass.sh record check | verify <sha>" >&2
        exit 2
        ;;
esac
