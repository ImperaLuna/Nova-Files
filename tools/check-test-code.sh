#!/usr/bin/env bash
# Fails on a test that asserts measured time or makes its own folder in the
# temp folder (rules and their tests in tools/ast-grep/test-code).
set -euo pipefail
source "$(dirname "$0")/code-dirs.sh"

readonly CONFIG=tools/ast-grep/test-code/sgconfig.yml

ast-grep test --config "$CONFIG" --skip-snapshot-tests
ast-grep scan --config "$CONFIG"
