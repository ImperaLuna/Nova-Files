#!/usr/bin/env bash
# Fails on a bare number outside a named constant (tools/ast-grep/rules).
set -euo pipefail
source "$(dirname "$0")/code-dirs.sh"
ast-grep scan --config tools/ast-grep/sgconfig.yml
