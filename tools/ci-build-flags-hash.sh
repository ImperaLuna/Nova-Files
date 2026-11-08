#!/usr/bin/env bash
# Prints `hash=<sha256>` of the settings that change how the mutation job's
# cargo builds come out: the mutants dev shell (linker, incremental) and the
# [profile.*] tables of the root Cargo.toml. CI keys its cargo cache on it.
# Dependencies stay out on purpose; rust-cache's own lock hash covers them.
set -euo pipefail

cd "$(dirname "$0")/.."
{
    cat nix/mutants-shell.nix
    # Every line from a [profile...] header up to the next other header.
    awk '/^\[/ { keep = /^\[profile(\.|\])/ } keep' Cargo.toml
} | sha256sum | cut -d' ' -f1 | sed 's/^/hash=/'
