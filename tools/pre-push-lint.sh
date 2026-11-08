#!/usr/bin/env bash
# Runs rustfmt and Clippy (all targets, warnings denied, as in CI) on the
# crates a push changes, so lint failures stop here instead of in CI.
#
# Usage: pre-push-lint.sh <remote> <sha>...
# Changes are taken since each sha's merge base with <remote>/main. A change
# to the root Cargo.toml or Cargo.lock lints the whole workspace. Without
# cargo on PATH (outside `nix develop`) it says so and lets the push through;
# CI still runs the same checks.
set -euo pipefail

remote=$1
shift
main="refs/remotes/$remote/main"

if ! command -v cargo >/dev/null; then
    echo "pre-push: cargo not found, skipping fmt and Clippy (run inside nix develop)." >&2
    exit 0
fi
git rev-parse -q --verify "$main" >/dev/null || exit 0
if [ -z "${IN_NIX_SHELL:-}" ]; then
    # A rustup or distro toolchain can be older than the flake's and miss
    # newer lints CI's Clippy has.
    echo "pre-push: not inside nix develop; $(cargo clippy --version 2>/dev/null || echo 'Clippy') may lack lints CI's has." >&2
fi

workspace=false
declare -A crates=()
for sha in "$@"; do
    base=$(git merge-base "$main" "$sha") || continue
    while IFS= read -r path; do
        case $path in
            Cargo.toml | Cargo.lock) workspace=true ;;
            crates/*/*) crate=${path#crates/}; crates[${crate%%/*}]=1 ;;
        esac
    done < <(git diff --no-renames --name-only "$base" "$sha")
done

root=$(git rev-parse --show-toplevel)
if $workspace; then
    scope=(--workspace)
else
    scope=()
    for crate in "${!crates[@]}"; do
        # A crate deleted by the push has nothing left to lint.
        [ -f "$root/crates/$crate/Cargo.toml" ] && scope+=(--package "$crate")
    done
    [ ${#scope[@]} -gt 0 ] || exit 0
fi

cd "$root"
echo "pre-push: rustfmt and Clippy on ${scope[*]}" >&2
if ! cargo fmt --all --check >&2; then
    echo "pre-push: run \`just fmt\`, then push again." >&2
    exit 1
fi
if ! cargo clippy "${scope[@]}" --all-targets -- --deny warnings >&2; then
    echo "pre-push: Clippy failed; fix it before pushing (CI runs the same check)." >&2
    exit 1
fi
