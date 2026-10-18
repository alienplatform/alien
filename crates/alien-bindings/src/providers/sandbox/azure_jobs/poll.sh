set -eu
job=$1 out=$2 err=$3
[ -d "$job" ] || exit 66
# Terminal state is sampled first: output published later belongs to the next poll.
state=running
[ ! -f "$job/exit" ] || state=$(cat "$job/exit")
reason=''
[ "$state" = running ] || reason=$(cat "$job/reason" 2>/dev/null || :)
truncated=false
[ ! -f "$job/truncated" ] || truncated=true
printf '%s\n%s\n%s\n' "$state" "$reason" "$truncated"
# A poll transfers at most 64 KiB from each stream; base64 preserves arbitrary bytes.
dd if="$job/stdout" bs=1 skip="$out" count=65536 2>/dev/null | base64 -w0
printf '\n'
dd if="$job/stderr" bs=1 skip="$err" count=65536 2>/dev/null | base64 -w0
printf '\n'
