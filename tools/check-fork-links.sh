#!/usr/bin/env bash
# Fails when a link to the GPUI fork (github.com/ImperaLuna/zed) points at
# another version than the one Cargo.toml pins, or when the gpui and
# gpui_platform pins differ. A kept comment that links the fork states a fact
# about the GPUI in use, so a move to a new pin rechecks each one.
set -euo pipefail

readonly FORK=https://github.com/ImperaLuna/zed

pin_of() { # <crate>
    sed -n "s/^$1 = { git = \"${FORK//\//\\/}\", rev = \"\([0-9a-f]*\)\".*/\1/p" Cargo.toml
}

stale_links() { # <rev>; file paths on stdin, NUL-separated; prints bad links
    xargs -0 grep -HnoE "${FORK}/(tree|blob)/[^[:space:]>)]*" |
        grep -vE "${FORK}/(tree|blob)/$1([^0-9a-f]|\$)" || true
}

expect_stale() { # <line> <rev> <count>
    local file got
    file=$(mktemp)
    printf '%s\n' "$1" >"$file"
    got=$(printf '%s\0' "$file" | stale_links "$2" | wc -l)
    rm -f "$file"
    if ((got != $3)); then
        echo "$0: '$1' against $2 should give $3 stale links, got $got." >&2
        exit 1
    fi
}

readonly SAMPLE_REV=2d5e9a92499dba81148304d079cd4ef00e54d0c5
expect_stale "// ${FORK}/tree/nova/v0.233.10/crates/gpui" "$SAMPLE_REV" 1
expect_stale "// ${FORK}/tree/${SAMPLE_REV}/crates/gpui" "$SAMPLE_REV" 0
expect_stale "// ${FORK}/blob/${SAMPLE_REV}/crates/gpui/src/app.rs" "$SAMPLE_REV" 0
expect_stale "// https://github.com/zed-industries/zed/issues/1" "$SAMPLE_REV" 0
expect_stale "/// <${FORK}/tree/${SAMPLE_REV}>." "$SAMPLE_REV" 0
expect_stale "// ${FORK}/tree/${SAMPLE_REV}/a and ${FORK}/tree/nova/v0.1/b" "$SAMPLE_REV" 1

gpui=$(pin_of gpui)
platform=$(pin_of gpui_platform)
if [[ -z "$gpui" || -z "$platform" ]]; then
    echo "$0: no gpui or gpui_platform pin on $FORK found in Cargo.toml; update this script." >&2
    exit 1
fi
if [[ "$gpui" != "$platform" ]]; then
    echo "Cargo.toml pins gpui at $gpui and gpui_platform at $platform; move both pins together."
    exit 1
fi

if [[ ! -f Cargo.toml ]]; then
    echo "$0: run it from the repo root." >&2
    exit 1
fi
stale=$(find . \( -name .git -o -name .claude -o -name target \) -prune -o -type f -not -name Cargo.lock -print0 |
    stale_links "$gpui")
if [[ -n "$stale" ]]; then
    echo "These links point at another GPUI than the pin ($gpui). Check each fact still holds there, then link $FORK/tree/$gpui/...:"
    echo "$stale"
    exit 1
fi
