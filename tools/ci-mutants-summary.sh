#!/usr/bin/env bash
# Fails a pull request whose description lacks the summary line of a clean
# local `just mutants` run. Agents run mutation tests once per PR before
# review (AGENTS.md). Without this check, a skipped run is first noticed by
# the CI mutation job, after 15 minutes. This check fails in seconds.
#
# Usage: tools/ci-mutants-summary.sh < pr-body
# Wants the line cargo-mutants prints last, for example
#   26 mutants tested in 9m: 20 caught, 6 unviable
# and refuses one that reports missed mutants.
set -euo pipefail

body=$(tr -d '\r')
summary=$(grep -oE '[0-9]+ mutants? tested in [^:]+: [a-z0-9, ]+' <<<"$body" | tail -n 1 || true)

if [ -z "$summary" ]; then
    echo "The PR description has no local mutation test summary. Run \`just mutants <base branch>\`, paste its last line (\"N mutants tested in ...: ...\") into the description, then rerun this job." >&2
    exit 1
fi
if grep -q missed <<<"$summary"; then
    echo "The local mutation run missed mutants: \"$summary\". Add tests (a mutant no test can ever catch goes in exclude_re in .cargo/mutants.toml, with a reason, through its own PR off main), run \`just mutants\` again, and paste the clean summary." >&2
    exit 1
fi
echo "Local mutation run: $summary"
