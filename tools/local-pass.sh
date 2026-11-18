#!/usr/bin/env bash
# Pass markers for the checks a push needs: `just check` and `just mutants`
# record one for the exact code they passed on, and the pre-push hook looks
# for them, so a push reaches CI only after both passed here.
#
# Usage:
#   local-pass.sh record check            after `nix flake check` passed
#   local-pass.sh record mutants <base>   after `just mutants <base>` passed
#   local-pass.sh verify <base> <sha>     exits 1, saying what to run, when
#                                         <sha> lacks a marker
#
# A check marker is keyed on the commit's tree. A mutants marker uses the
# key CI's cache uses (tools/ci-mutants-key.sh), from the merge base with
# <base> to the commit, so it holds while only <base> moves. Nothing is
# recorded while tracked files differ from HEAD: the markers name commits.
# They live in target/passed/, which `cargo clean` empties.
set -euo pipefail

root=$(git rev-parse --show-toplevel)
dir="$root/target/passed"

mutants_key() {
    local base=$1 sha=$2
    "$root/tools/ci-mutants-key.sh" "$(git merge-base "$base" "$sha")" "$sha" | sed 's/^key=//'
}

case ${1:-} in
    record)
        if ! git diff --quiet HEAD; then
            echo "Not recording a pass: tracked files differ from HEAD. Commit, then run it again." >&2
            exit 0
        fi
        mkdir -p "$dir"
        case ${2:-} in
            check) touch "$dir/check-$(git rev-parse 'HEAD^{tree}')" ;;
            mutants) touch "$dir/$(mutants_key "${3:?usage: local-pass.sh record mutants <base>}" HEAD)" ;;
            *) echo "usage: local-pass.sh record check|mutants <base>" >&2; exit 2 ;;
        esac
        ;;
    verify)
        base=${2:?usage: local-pass.sh verify <base> <sha>}
        sha=${3:?usage: local-pass.sh verify <base> <sha>}
        missing=()
        [ -e "$dir/check-$(git rev-parse "$sha^{tree}")" ] || missing+=("just check")
        if ! git diff --quiet "$(git merge-base "$base" "$sha")" "$sha" -- '*.rs' &&
            [ ! -e "$dir/$(mutants_key "$base" "$sha")" ]; then
            missing+=("just mutants ${base#refs/remotes/}")
        fi
        if [ ${#missing[@]} -gt 0 ]; then
            echo "pre-push: ${sha:0:12} has no local pass of: ${missing[*]/#/$'\n    '}" >&2
            echo "pre-push: run them on this commit, fix what fails, and push again." >&2
            exit 1
        fi
        ;;
    *)
        echo "usage: local-pass.sh record check|mutants <base> | verify <base> <sha>" >&2
        exit 2
        ;;
esac
