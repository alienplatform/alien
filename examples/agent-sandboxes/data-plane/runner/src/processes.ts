// Ending what a thread's turns left running.
//
// Nothing of the previous user needs to survive a user switch: every turn starts its own agent
// process and Claude Code keeps its conversation on disk. So the switch ends every process in
// the sandbox except the sandbox's own: PID 1, the processes that started the sweep itself, and
// kernel threads (no command line). The sweep does not look at anything the agent controls,
// such as environment variables or process names.
//
// `sweep` ends them and prints the pids. `list` prints what is left (and nothing when the
// sandbox is clean); the runner lists again after a sweep and refuses to go on if anything is
// still there.
//
// `$SWEEP_ROOT` (optional) limits both modes to the processes below one pid; the tests use it
// to run the script on a machine that has other processes.

export const SWEEP_SCRIPT = String.raw`
mode="$1"
self=$$
ppid_of() { sed -n 's/^PPid:[[:space:]]*//p' "/proc/$1/status" 2>/dev/null; }

# This script and the processes that started it stay.
keep=" "
p=$self
while [ -n "$p" ] && [ "$p" != 0 ]; do
  keep="$keep$p "
  p=$(ppid_of "$p")
done

# Prints the pids that are left to end.
candidates() {
  for d in /proc/[0-9]*; do
    pid=$(basename "$d")
    [ "$pid" = 1 ] && continue
    case "$keep" in *" $pid "*) continue ;; esac
    [ -n "$(tr -d '\000' < "$d/cmdline" 2>/dev/null)" ] || continue
    # Things this script started itself (ls, tr, sed) are not the sandbox's.
    p=$pid
    ours=
    while [ -n "$p" ] && [ "$p" != 0 ] && [ "$p" != 1 ]; do
      [ "$p" = "$self" ] && ours=1 && break
      p=$(ppid_of "$p")
    done
    [ -n "$ours" ] && continue
    if [ -n "$SWEEP_ROOT" ]; then
      p=$pid
      under=
      while [ -n "$p" ] && [ "$p" != 0 ]; do
        [ "$p" = "$SWEEP_ROOT" ] && under=1 && break
        p=$(ppid_of "$p")
      done
      [ -n "$under" ] || continue
    fi
    echo "$pid"
  done
}

if [ "$mode" = list ]; then
  candidates
  exit 0
fi

ended=" "
for round in 1 2 3 4 5; do
  left=$(candidates)
  [ -n "$left" ] || break
  for pid in $left; do
    kill -9 "$pid" 2>/dev/null && ended="$ended$pid "
  done
  sleep 0.2
done
for pid in $ended; do echo "$pid"; done
`
