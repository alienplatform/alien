set -eu
job=$1
[ -d "$job" ] || exit 66
[ ! -f "$job/exit" ] || exit 0
# Wait for a start receipt before claiming cancellation. A delayed fork must not leave a
# cancellation claim that prevents the independent timeout from ever firing.
timeout 5 sh -c 'while [ ! -f "$1/pid" ] && [ ! -f "$1/exit" ]; do sleep 0.1; done' sh "$job" || exit 75
[ ! -f "$job/exit" ] || exit 0
# Only the winner can stop this job. Completion and timeout use the same claim.
if mkdir "$job/stopping" 2>/dev/null; then
    pid=$(cat "$job/pid")
    case "$pid" in ''|*[!0-9]*) rmdir "$job/stopping"; exit 65;; esac
    echo cancelled > "$job/reason"
    kill -9 -"$pid" 2>/dev/null || :
fi
