#!/usr/bin/env bash
# Picks the mutants one CI shard tests, so a pull request with many mutants
# spreads them over several runners and one with few stays on one runner
# instead of paying every runner's setup.
#
# Usage: tools/ci-mutants-shard.sh <shard> <shards> <base>
#   shard   this runner's index, from 0
#   shards  how many runners the job started
#   base    the commit to diff against, as for `just mutants`
# Writes to $GITHUB_OUTPUT (stdout outside Actions):
#   skip=true            when this shard has nothing to do, or
#   skip=false and flags=--shard <shard>/<used>
# where <used> is one runner per MUTANTS_PER_SHARD mutants, at most <shards>.
# Run it inside `nix develop .#mutants`, after the cargo cache is restored:
# listing mutants reads the dependency graph but builds nothing.
set -euo pipefail

# About a minute and a half of mutants at 6-8 s each: below that a runner's
# own setup (about a minute) costs more than it saves.
MUTANTS_PER_SHARD=12

shard=${1:?usage: tools/ci-mutants-shard.sh <shard> <shards> <base>}
shards=${2:?usage: tools/ci-mutants-shard.sh <shard> <shards> <base>}
base=${3:?usage: tools/ci-mutants-shard.sh <shard> <shards> <base>}
out=${GITHUB_OUTPUT:-/dev/stdout}

mkdir -p target
git diff "$base"... -- '*.rs' >target/mutants.diff
count=0
if [ -s target/mutants.diff ]; then
    count=$(cargo mutants --list --in-diff target/mutants.diff --workspace | wc -l)
fi

used=$(((count + MUTANTS_PER_SHARD - 1) / MUTANTS_PER_SHARD))
used=$((used < 1 ? 1 : used))
used=$((used > shards ? shards : used))
echo "$count mutants over $used of $shards shards" >&2

if [ "$shard" -ge "$used" ]; then
    echo "skip=true" >>"$out"
else
    echo "skip=false" >>"$out"
    echo "flags=--shard $shard/$used" >>"$out"
fi
