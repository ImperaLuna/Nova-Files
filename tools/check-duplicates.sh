#!/usr/bin/env bash
# Fails when the same run of code appears twice in crates/ (copy-paste
# detection with PMD's CPD, which tokenises Rust).
#
# AGENTS.md: when logic shows up twice, move it into a shared function or
# component before adding a third copy. MIN_TOKENS is the shortest run
# that counts as a copy.
set -euo pipefail

readonly MIN_TOKENS=100

pmd cpd --language rust --minimum-tokens "$MIN_TOKENS" --dir crates
