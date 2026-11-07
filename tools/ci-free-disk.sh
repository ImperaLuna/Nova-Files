#!/usr/bin/env bash
# Frees disk space on a GitHub runner without holding up the job.
#
# `start` deletes preinstalled toolchains this repo never uses in the
# background, which takes 1 to 3 minutes, and records the deletion's exit
# status and output. `wait` blocks until it is done, up to WAIT_LIMIT, and
# fails with the deletion's output if it failed or ran out of time. Call it
# right before the step that needs the room, so the deletion overlaps the
# setup steps instead of running before them.
set -euo pipefail

# Five times the slowest deletion seen on a runner.
WAIT_LIMIT=15m

state=${RUNNER_TEMP:-/tmp}/free-disk
pid_file=$state.pid
status_file=$state.status
log_file=$state.log

case ${1:-} in
start)
    rm -f "$status_file"
    (
        status=0
        sudo rm -rf /usr/share/dotnet /usr/local/lib/android /opt/ghc /opt/hostedtoolcache/CodeQL ||
            status=$?
        echo "$status" >"$status_file"
    ) >"$log_file" 2>&1 &
    echo $! >"$pid_file"
    ;;
wait)
    # tail --pid returns once the deletion has exited.
    if ! timeout "$WAIT_LIMIT" tail --pid="$(cat "$pid_file")" -f /dev/null; then
        echo "Freeing disk space took longer than $WAIT_LIMIT." >&2
        cat "$log_file" >&2
        exit 1
    fi
    status=$(cat "$status_file")
    if [ "$status" != 0 ]; then
        echo "Freeing disk space failed with exit status $status:" >&2
        cat "$log_file" >&2
        exit 1
    fi
    df -h /
    ;;
*)
    echo "usage: tools/ci-free-disk.sh start|wait" >&2
    exit 2
    ;;
esac
