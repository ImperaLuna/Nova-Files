#!/usr/bin/env bash
# Fails on a Rust comment outside the keep list in AGENTS.md ("Comments"),
# or a kept one past two lines, wherever the file lives. The rules live in
# tools/ast-grep/comments; their tests run first. `--report` only counts.
set -euo pipefail

readonly CONFIG=tools/ast-grep/comments/sgconfig.yml
readonly API_CRATE=nova-files-api

ast-grep test --config "$CONFIG" --skip-snapshot-tests

files=$( (ast-grep scan --inline-rules '{id: rust-file, language: rust, rule: {kind: source_file}}' --json=stream . || true) | wc -l)
if ((files == 0)); then
    echo "No Rust files scanned: the comment lint covers nothing."
    exit 1
fi
if [[ -z "$(find . -path "*/$API_CRATE/Cargo.toml" -not -path './target/*' -print -quit)" ]]; then
    echo "No $API_CRATE crate found: the rules exempt its docs by that folder name; update them."
    exit 1
fi
echo "Scanned $files Rust files."

if [[ "${1:-}" == "--report" ]]; then
    count=$( (ast-grep scan --config "$CONFIG" --json=stream || true) | wc -l)
    echo "Comments outside the keep list: $count (report only)."
    exit 0
fi
ast-grep scan --config "$CONFIG"
