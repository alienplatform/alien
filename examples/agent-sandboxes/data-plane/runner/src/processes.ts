// Ending what a thread's turns left running.
//
// Nothing of the previous user needs to survive a user switch: every turn starts its own agent
// process and Claude Code keeps its conversation on disk. So the switch ends every process of
// the sandbox user (the user commands run as), in two separate commands:
//
// 1. SWEEP_SCRIPT runs `kill -9 -1`. The kernel signals every process this user may signal,
//    except the caller, in one call under the task list lock: nothing can fork away, rename
//    itself or be re-parented in between, and no command line, name or environment is read.
//    It may end the command's own wrapper too, so the command's exit status means nothing.
// 2. VERIFY_SCRIPT, a new command, lists every process of that user except its own ancestors
//    (zombies, which hold nothing, are skipped). It exits 1 and prints the pids if any is left,
//    and exits 2 if anything cannot be read. The turn starts only when it exits 0.
//
// Both refuse to run as root (exit 1, "must run as the sandbox user, not root"): `kill -1` as
// root would end the runtime's own helpers. A runtime that keeps a long-lived process as the
// sandbox user is ended too, and the switch then fails closed: the turn errors. So the sandbox
// image must run commands as a non-root user that nothing else in the sandbox uses.
//
// `$PROC_DIR` (optional, tests only) is the directory the verify lists, so a test can show how
// it behaves for a process it cannot read.

export const SWEEP_SCRIPT = String.raw`
me=$(sed -n 's/^Uid:[[:space:]]*\([0-9]*\).*/\1/p' /proc/$$/status)
[ -n "$me" ] || { echo "cannot read the sweep's own uid" >&2; exit 1; }
[ "$me" != 0 ] || { echo "the sweep must run as the sandbox user, not root" >&2; exit 1; }
kill -9 -1
`

// Shell builtins only: no process of its own is started, so there is nothing of the verify's
// own to tell apart from a leftover.
// (A plain template, not String.raw: `\${` is how the shell's own `${` is written in it.)
export const VERIFY_SCRIPT = `
proc="\${PROC_DIR:-/proc}"
uid_of() { # prints the real uid from a status file
  while read -r key val _; do
    if [ "$key" = "Uid:" ]; then echo "$val"; return 0; fi
  done < "$1"
  return 1
}
ppid_of() {
  while read -r key val _; do
    if [ "$key" = "PPid:" ]; then echo "$val"; return 0; fi
  done < "$1"
  return 1
}
me=$(uid_of /proc/$$/status) || { echo "cannot read the verify's own uid" >&2; exit 2; }
[ "$me" != 0 ] || { echo "the verify must run as the sandbox user, not root" >&2; exit 1; }

# The verify's own ancestors (the shell it runs in and what started that) are not leftovers.
keep=" "
p=$$
while [ -n "$p" ] && [ "$p" != 0 ]; do
  keep="$keep$p "
  p=$(ppid_of /proc/$p/status) || { echo "cannot read the parent of $p" >&2; exit 2; }
done

left=
bad=
for d in "$proc"/[0-9]*; do
  pid=\${d##*/}
  case "$keep" in *" $pid "*) continue ;; esac
  if ! uid=$(uid_of "$d/status" 2>/dev/null); then
    [ -d "$d" ] || continue   # it exited since the listing
    bad="$bad $pid"; continue
  fi
  [ "$uid" = "$me" ] || continue
  if ! read -r line < "$d/stat" 2>/dev/null; then
    [ -d "$d" ] || continue
    bad="$bad $pid"; continue
  fi
  # Field 2 (the command name) may hold spaces and parentheses: the state is the first field
  # after the LAST closing parenthesis.
  rest=\${line##*) }
  state=\${rest%% *}
  [ "$state" = Z ] && continue
  left="$left $pid"
done
if [ -n "$bad" ]; then echo "cannot read:$bad" >&2; exit 2; fi
if [ -n "$left" ]; then for pid in $left; do echo "$pid"; done; exit 1; fi
exit 0
`
