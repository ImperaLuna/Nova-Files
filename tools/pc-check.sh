#!/usr/bin/env bash
# Runs `just check` on a claude/* branch in the shared checkout, capped so
# the PC stays usable, then pushes the branch when --push is given and the
# check passed. Holds .git/freeze.lock for the whole run and switches back
# to where the checkout was.
#
# Usage:
#   tools/pc-check.sh <claude/branch> [--push]
set -euo pipefail

branch=${1:?usage: tools/pc-check.sh <claude/branch> [--push]}
push=${2:-}
case $branch in
    claude/*) ;;
    *) echo "pc-check: only claude/* branches, got $branch" >&2; exit 2 ;;
esac

cd "$(git rev-parse --show-toplevel)"
exec 9>.git/freeze.lock
flock 9

if [ -n "$(git status --porcelain --untracked-files=no)" ]; then
    echo "pc-check: tracked changes in the checkout, nothing touched" >&2
    exit 3
fi

if start=$(git symbolic-ref -q --short HEAD); then
    trap 'git switch -q "$start"' EXIT
else
    start=$(git rev-parse HEAD)
    trap 'git switch -q --detach "$start"' EXIT
fi

git fetch -q origin "$branch" 2>/dev/null || true
if git show-ref -q --verify "refs/heads/$branch"; then
    git switch -q "$branch"
else
    git switch -q -c "$branch" --track "origin/$branch"
fi
echo "pc-check: $branch at $(git rev-parse --short HEAD)"

cores=$(( $(nproc) / 2 ))
systemd-run --user --scope --quiet \
    -p CPUWeight=idle -p IOWeight=10 -p MemoryMax=6G -p MemorySwapMax=2G \
    env TZ=UTC CARGO_BUILD_JOBS="$cores" \
    NIX_CONFIG="cores = $cores"$'\n'"max-jobs = 1" \
    nice -n 19 nix develop -c just check

if [ "$push" = --push ]; then
    git push -u origin "$branch"
fi
