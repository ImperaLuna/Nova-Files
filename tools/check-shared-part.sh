#!/usr/bin/env bash
# Prints how many lines a pull request adds to framework/, and fails when it
# grows the framework (outside tests) without a "## Shared part" section in
# its description that says what it adds and why that can't live in the
# feature.
#
# Usage: git diff --numstat <base> <head> -- framework/ | tools/check-shared-part.sh
# The description comes in $PR_BODY. `--self-test` runs only the tests,
# built from pull request #340, which put the archive password prompt in
# framework/window without saying so.
set -euo pipefail

readonly FIXTURES=tools/shared-part

growth() { # numstat on stdin; prints "<added> <removed> <net outside tests>"
    awk -F'\t' '
        $1 == "-" { next }
        { added += $1; removed += $2 }
        $3 !~ /(^|\/)tests(\.rs$|\/)/ { net += $1 - $2 }
        END { printf "%d %d %d\n", added, removed, net }
    '
}

names_shared_part() { # body on stdin
    awk '
        /^#+[[:space:]]+Shared part[[:space:]]*$/ { inside = 1; next }
        inside && /^#/ { exit }
        inside && /[^[:space:]]/ { found = 1; exit }
        END { exit !found }
    '
}

verdict() { # <numstat file> <body file>
    local added removed net
    read -r added removed net < <(growth <"$1")
    echo "Lines this pull request adds to framework/: +$added -$removed ($net net outside tests)."
    if ((net > 0)) && ! names_shared_part <"$2"; then
        echo "It grows the framework: add a '## Shared part' section to the description saying what it adds and why it can't live in the feature."
        return 1
    fi
}

expect() { # <passes|fails> <numstat file> <body file>
    local got=passes
    verdict "$2" "$3" >/dev/null || got=fails
    if [[ "$got" != "$1" ]]; then
        echo "$0: $2 with $3 should be '$1', got '$got'." >&2
        exit 1
    fi
}

self_test() {
    local empty
    empty=$(mktemp)
    trap 'rm -f "$empty"' RETURN
    expect fails "$FIXTURES/pr340.numstat" "$FIXTURES/pr340-body.md"
    expect passes "$FIXTURES/pr340.numstat" "$FIXTURES/shared-part-body.md"
    expect fails "$FIXTURES/pr340.numstat" "$FIXTURES/empty-section-body.md"
    expect passes "$FIXTURES/tests-only.numstat" "$empty"
    expect passes "$FIXTURES/shrinks.numstat" "$empty"
    expect passes "$empty" "$empty"
}

self_test
if [[ "${1:-}" == "--self-test" ]]; then
    exit 0
fi

numstat=$(mktemp)
body=$(mktemp)
trap 'rm -f "$numstat" "$body"' EXIT
cat >"$numstat"
printf '%s\n' "${PR_BODY:-}" >"$body"
verdict "$numstat" "$body"
