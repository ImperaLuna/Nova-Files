#!/usr/bin/env bash
# Runs a command capped so the machine stays usable: half the cores, one
# Nix build at a time, idle CPU and low IO priority, 6 GB of memory. The
# Nix settings reach the daemon's builds; the cgroup limits hold only for
# processes this script starts, so the daemon needs its own limits too.
#
# Usage:
#   tools/capped.sh <command> [args...]
set -euo pipefail

cores=$(( $(nproc) / 2 ))
(( cores > 0 )) || cores=1
export CARGO_BUILD_JOBS=$cores
export NEXTEST_TEST_THREADS=$cores
export RUST_TEST_THREADS=$cores
export NIX_CONFIG="${NIX_CONFIG:+$NIX_CONFIG$'\n'}cores = $cores"$'\n'"max-jobs = 1"

if systemd-run --user --scope --quiet true 2>/dev/null; then
    exec systemd-run --user --scope --quiet \
        -p CPUWeight=idle -p IOWeight=10 -p "CPUQuota=$(( cores * 100 ))%" \
        -p MemoryMax=6G -p MemorySwapMax=2G \
        nice -n 19 "$@"
fi
exec nice -n 19 "$@"
