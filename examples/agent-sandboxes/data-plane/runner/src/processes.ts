// Ending what a user's turns left running.
//
// Every command the runner starts for a user carries `SANDBOX_TURN_USER=<user>` in its
// environment, and a child process inherits it. When the thread changes hands, a script run in
// the sandbox ends every process that carries the previous user's mark. Claude Code itself is
// one process per turn and keeps its conversation on disk, so the agent session needs nothing
// to stay running between turns.

export const TURN_USER_VAR = "SANDBOX_TURN_USER"

/** Reads the owner's name from `$OWNER` (never from the script text) and prints the ended pids. */
export const END_PROCESSES_SCRIPT = [
  "for pid in $(ls /proc); do",
  '  case "$pid" in *[!0-9]*) continue ;; esac',
  '  [ "$pid" = "$$" ] && continue',
  `  if tr '\\0' '\\n' < "/proc/$pid/environ" 2>/dev/null | grep -qxF "${TURN_USER_VAR}=$OWNER"; then`,
  '    kill -9 "$pid" 2>/dev/null && echo "$pid"',
  "  fi",
  "done",
  "true",
].join("\n")
