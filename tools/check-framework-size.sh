#!/usr/bin/env bash
# Fails when framework/base or what framework/shell offers outgrows its
# ceiling. Test files (tests.rs, tests/) don't count; inline test modules
# do. Each ceiling sits about 10% over the size when it was set, so ordinary
# work fits; past it, move the new part into the feature that needs it, or
# raise the ceiling in a pull request that says why.
set -euo pipefail

readonly BASE_CEILING=2100
readonly SHELL_CEILING=1250

code_files() { # <folder>
    find "$1" -name '*.rs' -not -name tests.rs -not -path '*/tests/*' -print0
}

base_lines() {
    code_files framework/base | xargs -0 cat | wc -l
}

shell_surface() {
    code_files framework/shell |
        xargs -0 grep -hE '^[[:space:]]*pub ' | wc -l
}

within() { # <what> <count> <ceiling>
    if (($2 > $3)); then
        echo "$1: $2 (ceiling $3). Keep feature parts in the feature, or raise the ceiling and say why in the PR."
        return 1
    fi
    echo "$1: $2 (ceiling $3)"
}

expect() { # <passes|fails> <count> <ceiling>
    local got=passes
    within test "$2" "$3" >/dev/null || got=fails
    if [[ "$got" != "$1" ]]; then
        echo "$0: $2 against ceiling $3 should be '$1', got '$got'." >&2
        exit 1
    fi
}

expect fails 1948 1500
expect passes 1948 "$BASE_CEILING"
expect passes "$BASE_CEILING" "$BASE_CEILING"
expect fails $((BASE_CEILING + 1)) "$BASE_CEILING"

if [[ ! -d framework/base || ! -d framework/shell ]]; then
    echo "$0: run it from the repo root." >&2
    exit 1
fi

status=0
within "Lines in framework/base" "$(base_lines)" "$BASE_CEILING" || status=1
within "Public lines in framework/shell" "$(shell_surface)" "$SHELL_CEILING" || status=1
exit "$status"
