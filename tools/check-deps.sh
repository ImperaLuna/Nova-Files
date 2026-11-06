#!/usr/bin/env bash
# Fails when a crate depends on a crate its Cargo.toml doesn't allow.
#
# Each crates/*/Cargo.toml opens with two comment lines:
#   # Allowed outside crates: rustix, serde      (or "none")
#   # Allowed workspace crates: nova-files-api   (or "none")
# This compares them with the normal and build dependencies that
# `cargo metadata` reports. Dev-dependencies are left to cargo-deny.
set -euo pipefail

allowed_list() { # <Cargo.toml> <label>
    local line
    line=$(grep -m1 "^# Allowed $2 crates:" "$1" || true)
    if [[ -z "$line" ]]; then
        echo "MISSING"
        return
    fi
    line=${line#*:}
    line=${line//,/ }
    if [[ "$line" =~ ^[[:space:]]*none[[:space:]]*$ ]]; then
        line=""
    fi
    echo "$line"
}

metadata=$(cargo metadata --no-deps --offline --format-version 1)
workspace_names=$(jq -r '.packages[].name' <<<"$metadata")

status=0
while IFS=$'\t' read -r name manifest deps; do
    outside=$(allowed_list "$manifest" outside)
    inside=$(allowed_list "$manifest" workspace)
    if [[ "$outside" == MISSING || "$inside" == MISSING ]]; then
        echo "$manifest: needs '# Allowed outside crates:' and '# Allowed workspace crates:' lines"
        status=1
        continue
    fi
    for dep in $deps; do
        if grep -qxF "$dep" <<<"$workspace_names"; then
            allowed=$inside
            kind=workspace
        else
            allowed=$outside
            kind=outside
        fi
        if ! tr -s ' ' '\n' <<<"$allowed" | grep -qxF -- "$dep"; then
            echo "$name depends on $dep, which is not in its allowed $kind crates ($manifest)."
            echo "  If it is really needed, add it to that line and say why in the PR."
            status=1
        fi
    done
done < <(jq -r '.packages[] | [.name, .manifest_path,
    ([.dependencies[] | select(.kind != "dev") | .name] | unique | join(" "))] | @tsv' <<<"$metadata")
exit "$status"
