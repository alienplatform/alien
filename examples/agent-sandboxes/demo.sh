#!/usr/bin/env bash
# Drives the demo moments through the control plane's HTTP API, the same calls the UI makes.
#   CONTROL_PLANE=http://localhost:3000 INTERNAL_API_IP=<private ip> SCANNER_IP=<private ip> ./demo.sh
# Moments 1-2 run commands in the sandbox directly. Moments 3-6 go through the agent and need a model.
set -euo pipefail

CP=${CONTROL_PLANE:-http://localhost:3000}
INTERNAL=${INTERNAL_API_IP:-internal-api}
IDLE_WAIT=${IDLE_WAIT:-180}   # a little over IDLE_PAUSE_SECONDS

json() { python3 -c 'import json,sys; print(json.dumps(dict(zip(sys.argv[1::2], sys.argv[2::2]))))' "$@"; }
post() {
  local out code
  out=$(curl -sS -w '\n%{http_code}' -X POST "$CP$1" -H 'content-type: application/json' --data "$2") || return 1
  code=${out##*$'\n'}; out=${out%$'\n'*}
  echo "$out"
  [ "${code:0:1}" = 2 ] || { echo "request failed ($code): $1" >&2; return 1; }
}
say() { printf '\n== %s\n' "$*"; }

# Sends a message; a thread answers 409 while its previous turn is still ending, so retry briefly.
say_to() {
  local out code
  for _ in $(seq 30); do
    out=$(curl -sS -w '\n%{http_code}' -X POST "$CP/api/threads/$1/messages" -H 'content-type: application/json' --data "$(json user "$2" text "$3")") || return 1
    code=${out##*$'\n'}
    [ "${code:0:1}" = 2 ] && return 0
    [ "$code" = 409 ] || break
    sleep 1
  done
  echo "could not send ($code): ${out%$'\n'*}" >&2; return 1
}

# Prints the thread's events from the start; stops after $2 seconds.
events() { curl -sN -m "$2" "$CP/api/threads/$1/events" || true; }
# Waits until the thread's history contains $2 (a fixed string), for at most $3 seconds.
wait_for() {
  for _ in $(seq "$3"); do
    events "$1" 1 | grep -qF -- "$2" && return 0
  done
  echo "timed out waiting for: $2" >&2; return 1
}
last_result() { events "$1" 1 | grep '"type":"result"' | tail -1 | sed -E 's/^data: //' | python3 -c 'import json,sys; print(json.load(sys.stdin)["event"].get("result"))'; }
exec_in() { post "/api/threads/$1/exec" "$(json command "$2")" >/dev/null; sleep 4; events "$1" 1 | grep '"type":"exec"' | tail -1; }

THREAD=$(post /api/threads "$(json title "demo $(date +%H:%M:%S)" user alice)" | python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])')
echo "thread $THREAD"

say "1. Only placeholder keys"
exec_in "$THREAD" 'env | grep -E "KEY|TOKEN"'

say "2. Nothing else is reachable"
exec_in "$THREAD" "curl -sS -m 8 -w ' [%{http_code}]' https://example.com; curl -sS -m 8 -w ' [%{http_code}]' http://$INTERNAL:8080/orders"

[ "${WITH_MODEL:-1}" = 1 ] || { echo; echo "WITH_MODEL=0: skipping the agent moments"; exit 0; }

say "2b. The same data through the connector, asked of the agent"
say_to "$THREAD" alice 'List the orders.'
wait_for "$THREAD" '"type":"result"' 120
last_result "$THREAD"

say "3. A write waits for approval"
say_to "$THREAD" alice 'Refund order 1003.'
wait_for "$THREAD" '"pending":[{' 120
echo "(the UI shows Approve / Deny for this held write; approving through the API)"
APPROVAL=$(events "$THREAD" 1 | grep '"pending":\[{' | tail -1 | sed -E 's/^data: //' | python3 -c 'import json,sys; p=json.load(sys.stdin)["pending"][0]; print(p["id"]); print(p["method"], p["path"], p["body"], file=sys.stderr)')
echo "alice cannot approve her own write:"
post "/api/threads/$THREAD/approvals/$APPROVAL" "{\"approve\":true,\"by\":\"alice\"}"
echo "a manager can:"
post "/api/threads/$THREAD/approvals/$APPROVAL" "{\"approve\":true,\"by\":\"manager\"}"

say "4. Switch the user mid-thread"
sleep 20
echo "a process alice's session leaves running:"
exec_in "$THREAD" 'nohup sleep 600 >/dev/null 2>&1 & echo started'
say_to "$THREAD" bob 'Cancel order 1004.'
sleep 40
events "$THREAD" 1 | grep -E 'user switched|bob|ended' | tail -5
echo "alice's leftover process is gone:"
exec_in "$THREAD" 'pgrep -fx "sleep 600" || echo "none running"'

say "5. A key the sandbox never holds"
say_to "$THREAD" alice 'List the scanner findings.'
sleep 40
last_result "$THREAD"
echo "the sandbox's environment has no scanner key:"
exec_in "$THREAD" 'env | grep -ci scanner || true'
SCANNER=${SCANNER_IP:-scanner}
echo "and the scanner is not reachable from it:"
exec_in "$THREAD" "curl -sS -m 8 -w ' [%{http_code}]' http://$SCANNER:8081/findings"

say "6. Idle, pause, resume with the agent process alive"
say_to "$THREAD" alice 'Start `orders watch` in the background and tell me its pid.'
sleep 40
echo "waiting $IDLE_WAIT s for the thread to pause"; sleep "$IDLE_WAIT"
curl -fsS "$CP/api/threads" | python3 -c 'import json,sys; [print(t["id"], t["state"]) for t in json.load(sys.stdin)["threads"]]'
say_to "$THREAD" alice 'Is the orders watch still running? Check with pgrep and give the pid.'
sleep 40
last_result "$THREAD"
