#!/usr/bin/env bash
# Tells a CI job whether the pull request changes anything the job checks,
# so a job with nothing to check reports success in seconds. GitHub requires
# both jobs by name, so they always run; only their work is skipped.
#
# Usage: tools/ci-needs-run.sh rust|code
#   rust  any .rs file: the mutation job mutates only changed Rust lines.
#   code  anything but Markdown, LICENSE and .gitignore: nix flake check.
# Writes run=true or run=false to $GITHUB_OUTPUT (stdout outside Actions).
# Outside a pull request, such as a push to main, the answer is always true.
set -euo pipefail

mode=${1:?usage: tools/ci-needs-run.sh rust|code}
out=${GITHUB_OUTPUT:-/dev/stdout}

if [ "${GITHUB_EVENT_NAME:-}" != "pull_request" ]; then
    echo "run=true" >>"$out"
    exit 0
fi

# A pull request's checkout is GitHub's merge commit. Its first parent is
# the base branch, so HEAD^1..HEAD is exactly what the pull request changes.
changed=$(git diff --name-only HEAD^1 HEAD)

case $mode in
rust) relevant=$(grep -E '\.rs$' <<<"$changed" || true) ;;
code) relevant=$(grep -vE '(\.md|^LICENSE|^\.gitignore)$' <<<"$changed" || true) ;;
*)
    echo "unknown mode: $mode" >&2
    exit 2
    ;;
esac

if [ -n "$relevant" ]; then
    echo "run=true" >>"$out"
else
    echo "Nothing for this job in the changed files:"
    echo "$changed"
    echo "run=false" >>"$out"
fi
