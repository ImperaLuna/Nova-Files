#!/usr/bin/env bash
# Frees disk space on a GitHub runner without holding up the job.
#
# `start` deletes preinstalled toolchains this repo never uses
# in the background, which takes 1 to 3 minutes. `wait` blocks until that
# is done; call it right before the step that needs the room, so the
# deletion overlaps the setup steps instead of running before them.
set -euo pipefail

pid_file=${RUNNER_TEMP:-/tmp}/free-disk.pid

case ${1:-} in
start)
    sudo rm -rf /usr/share/dotnet /usr/local/lib/android /opt/ghc /opt/hostedtoolcache/CodeQL \
        >/dev/null 2>&1 &
    echo $! >"$pid_file"
    ;;
wait)
    # tail --pid returns once the deletion has exited.
    tail --pid="$(cat "$pid_file")" -f /dev/null
    df -h /
    ;;
*)
    echo "usage: tools/ci-free-disk.sh start|wait" >&2
    exit 2
    ;;
esac
