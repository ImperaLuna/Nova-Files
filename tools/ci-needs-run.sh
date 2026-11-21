#!/usr/bin/env bash
# Tells the check job whether the pull request changes anything it checks
# (anything but Markdown, LICENSE and .gitignore), so with nothing to check
# it reports success in seconds. GitHub requires the job by name, so it
# always runs; only its work is skipped.
#
# Usage: tools/ci-needs-run.sh code
# Writes run=true or run=false to $GITHUB_OUTPUT (stdout outside Actions).
# Outside a pull request, such as a push to main, the answer is always true.
set -euo pipefail

mode=${1:?usage: tools/ci-needs-run.sh code}
out=${GITHUB_OUTPUT:-/dev/stdout}

if [ "${GITHUB_EVENT_NAME:-}" != "pull_request" ]; then
    echo "run=true" >>"$out"
    exit 0
fi

# A pull request's checkout is GitHub's merge commit. Its first parent is
# the base branch, so HEAD^1..HEAD is exactly what the pull request changes.
changed=$(git diff --name-only HEAD^1 HEAD)

case $mode in
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
