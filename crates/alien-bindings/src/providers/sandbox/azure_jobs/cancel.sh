set -eu
job=$1
[ -d "$job" ] || exit 66
[ ! -f "$job/exit" ] || exit 0
# Only the winner can stop this job. Completion and timeout use the same claim.
if mkdir "$job/stopping" 2>/dev/null; then
    echo cancelled > "$job/reason"
    # The supervisor publishes pid before the timer; a caller can arrive just before it.
    for attempt in 1 2 3 4 5; do
        [ ! -f "$job/pid" ] || break
        sleep 0.1
    done
    pid=$(cat "$job/pid")
    case "$pid" in ''|*[!0-9]*) exit 65;; esac
    kill -9 -"$pid" 2>/dev/null || :
fi
