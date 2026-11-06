#!/usr/bin/env bash
# Fails when a crate's clippy.toml misses a line of the root clippy.toml.
#
# Clippy reads only the nearest clippy.toml and never merges, so a crate
# with its own file must repeat every root setting. This compares lines
# (comments and blank lines skipped) and lists the ones a crate file lacks.
set -euo pipefail

root=clippy.toml
status=0
for crate_config in crates/*/clippy.toml; do
    [[ -e "$crate_config" ]] || continue
    missing=$(grep -vE '^\s*(#|$)' "$root" | grep -vxFf "$crate_config" || true)
    if [[ -n "$missing" ]]; then
        echo "$crate_config is missing these lines from $root:"
        echo "$missing"
        status=1
    fi
done
exit "$status"
