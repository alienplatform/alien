// Ending what a thread's turns left running.
//
// Nothing of the previous user needs to survive a user switch: every turn starts its own agent
// process and Claude Code keeps its conversation on disk. So the switch ends every process in
// the sandbox that belongs to the sandbox user: the user the sweep itself runs as. Processes of
// other users (the runtime's own, which the sandbox starts as root) are not the agent's and stay,
// as do PID 1, the sweep's own ancestors, what the sweep's own command started in the last 5 seconds, and kernel threads (no command
// line). The owner comes from the kernel (the Uid line of /proc/<pid>/status), which the agent
// cannot change; the sweep does not look at anything else the agent controls, such as
// environment variables or process names.
//
// `sweep` ends them and prints the pids. `list` prints what is left (and nothing when the
// sandbox is clean); the runner lists again after a sweep and refuses to go on if anything is
// still there.
//
// Run as root, the sweep refuses (exit 1, "the sweep must run as the sandbox user, not root"):
// it would end the runtime's own helpers. The runner then fails the switch and no turn starts.
//
// `$SWEEP_ROOT` (optional) limits both modes to the processes below one pid; the tests use it
// to run the script on a machine that has other processes.

export const SWEEP_SCRIPT = String.raw`
mode="$1"
self=$$
me=$(sed -n 's/^Uid:[[:space:]]*\([0-9]*\).*/\1/p' /proc/$$/status)
[ -n "$me" ] || { echo "cannot read the sweep's own uid" >&2; exit 1; }
# Root's processes include the runtime's own helpers: never end them. The switch fails closed.
[ "$me" != 0 ] || { echo "the sweep must run as the sandbox user, not root" >&2; exit 1; }
ppid_of() { sed -n 's/^PPid:[[:space:]]*//p' "/proc/$1/status" 2>/dev/null; }

# This script and the processes that started it stay.
keep=" "
p=$self
while [ -n "$p" ] && [ "$p" != 0 ]; do
  keep="$keep$p "
  p=$(ppid_of "$p")
done

# The command that started the sweep (the runtime wraps every command in a shell with a watchdog
# beside it) is the topmost ancestor below PID 1. What it started at the same time as the sweep
# (the watchdog, ls, sed) is the sweep's own. A runtime that runs every command under one
# long-lived parent has old background processes under that parent too: those are not spared.
top=$self
p=$(ppid_of "$self")
while [ -n "$p" ] && [ "$p" != 0 ] && [ "$p" != 1 ]; do
  top=$p
  p=$(ppid_of "$p")
done

# Start time in clock ticks since boot (field 22 of /proc/<pid>/stat; the command name may hold
# spaces, so cut after its closing parenthesis).
started() { sed 's/.*) //' "/proc/$1/stat" 2>/dev/null | cut -d' ' -f20; }
hz=$(getconf CLK_TCK 2>/dev/null || echo 100)
since=$(( $(started "$self") - 5 * hz ))

# Prints the pids that are left to end.
candidates() {
  for d in /proc/[0-9]*; do
    pid=$(basename "$d")
    [ "$pid" = 1 ] && continue
    case "$keep" in *" $pid "*) continue ;; esac
    [ -n "$(tr -d '\000' < "$d/cmdline" 2>/dev/null)" ] || continue
    # Only the sandbox user's processes (the real uid, set by the kernel).
    [ "$(sed -n 's/^Uid:[[:space:]]*\([0-9]*\).*/\1/p' "$d/status" 2>/dev/null)" = "$me" ] || continue
    # Things the sweep's own command started just now (ls, tr, sed, the runtime's watchdog) are
    # not the sandbox's. Turns are serialized and a switch happens between turns, so nothing
    # legitimate is more than 5 seconds older than the sweep.
    p=$pid
    ours=
    while [ -n "$p" ] && [ "$p" != 0 ] && [ "$p" != 1 ]; do
      [ "$p" = "$top" ] && ours=1 && break
      p=$(ppid_of "$p")
    done
    if [ -n "$ours" ]; then
      t=$(started "$pid")
      [ -n "$t" ] && [ "$t" -ge "$since" ] && continue
    fi
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
