# Arguments: root, job id, timeout seconds, cwd, command and arguments.
set -eu
root=$1 id=$2 timeout=$3 cwd=$4
shift 4
for tool in setsid nohup head cat mkfifo base64 dd timeout flock; do
    command -v "$tool" >/dev/null || exit 69
done
umask 077
mkdir -p "$root"
# Kernel-owned lock releases on exec failure or disconnect, without a stale lock directory.
exec 9> "$root/lock"
flock -w 5 9 || exit 75
count=0
for old in "$root"/*/; do
    [ -d "$old" ] || continue
    count=$((count + 1))
done
if [ "$count" -ge 16 ]; then
    for old in "$root"/*/; do
        if [ -f "$old/delivered" ]; then rm -rf "$old"; count=$((count - 1)); break; fi
    done
fi
[ "$count" -lt 16 ] || exit 76
job=$root/$id
mkdir "$job"
mkfifo "$job/out.pipe" "$job/err.pipe"
# The detached supervisor owns the pipes, timer and completion record after exec returns.
nohup sh -c '
job=$1 timeout=$2 cwd=$3
shift 3
cd "$cwd" || { echo 125 > "$job/exit"; exit; }
(head -c 4194304; if [ -n "$(head -c 1 | base64)" ]; then touch "$job/truncated"; fi; cat >/dev/null) <"$job/out.pipe" >"$job/stdout" & out=$!
(head -c 4194304; if [ -n "$(head -c 1 | base64)" ]; then touch "$job/truncated"; fi; cat >/dev/null) <"$job/err.pipe" >"$job/stderr" & err=$!
setsid sh -c '\''job=$1; shift; echo "$$" > "$job/pid"; exec "$@"'\'' sh "$job" "$@" < /dev/null >"$job/out.pipe" 2>"$job/err.pipe" & child=$!
# A separate session keeps the timer out of the command process group.
setsid sh -c '\''sleep "$1"; if mkdir "$2/stopping" 2>/dev/null; then echo timeoutExceeded > "$2/reason"; kill -9 -"$3" 2>/dev/null; fi'\'' sh "$timeout" "$job" "$child" & timer=$!
wait "$child"; code=$?
kill -9 -"$timer" 2>/dev/null
wait "$timer" 2>/dev/null
# Claim completion before a late cancel can signal a recycled PID.
mkdir "$job/stopping" 2>/dev/null || :
wait "$out" "$err"
printf "%s\n" "$code" > "$job/exit.tmp"
mv "$job/exit.tmp" "$job/exit"
' sh "$job" "$timeout" "$cwd" "$@" 9>&- </dev/null >/dev/null 2>&1 &
printf '%s\n' "$id"
