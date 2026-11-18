#!/usr/bin/env bash
# Fails on a comment in crates/ outside the keep list in AGENTS.md
# ("Comments"), or a kept one past two lines. The rules live in
# tools/ast-grep/comments; their tests run first. `--report` only counts.
set -euo pipefail

readonly CONFIG=tools/ast-grep/comments/sgconfig.yml

ast-grep test --config "$CONFIG" --skip-snapshot-tests

if [[ "${1:-}" == "--report" ]]; then
    count=$( (ast-grep scan --config "$CONFIG" --json=stream || true) | wc -l)
    echo "Comments outside the keep list: $count (report only)."
    exit 0
fi
ast-grep scan --config "$CONFIG"
