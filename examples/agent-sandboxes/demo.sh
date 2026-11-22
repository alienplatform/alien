#!/usr/bin/env bash
# Drives the demo moments through the control plane's HTTP API, the same calls the UI makes.
#   CONTROL_PLANE=http://localhost:3000 INTERNAL_API_IP=<private ip> ./demo.sh
# Moments 1-2 run commands in the sandbox directly. Moments 3-5 go through the agent and need a model.
set -euo pipefail

CP=${CONTROL_PLANE:-http://localhost:3000}
INTERNAL=${INTERNAL_API_IP:-internal-api}
IDLE_WAIT=${IDLE_WAIT:-180}   # a little over IDLE_PAUSE_SECONDS

json() { python3 -c 'import json,sys; print(json.dumps(dict(zip(sys.argv[1::2], sys.argv[2::2]))))' "$@"; }
post() { curl -fsS -X POST "$CP$1" -H 'content-type: application/json' --data "$2"; echo; }
say() { printf '\n== %s\n' "$*"; }

# Prints the thread's events from the start; stops after $2 seconds.
events() { curl -sN -m "$2" "$CP/api/threads/$1/events" || true; }
# Waits until the thread's history contains $2 (a fixed string), for at most $3 seconds.
wait_for() {
  for _ in $(seq "$3"); do
    events "$1" 1 | grep -qF -- "$2" && return 0
  done
  echo "timed out waiting for: $2" >&2; return 1
}
last_result() { events "$1" 1 | grep '"event":{"type":"result"' | tail -1 | sed -E 's/^data: //' | python3 -c 'import json,sys; print(json.load(sys.stdin)["event"].get("result"))'; }
exec_in() { post "/api/threads/$1/exec" "$(json command "$2")" >/dev/null; sleep 4; events "$1" 1 | grep '"type":"exec"' | tail -1; }

THREAD=$(post /api/threads "$(json title "demo $(date +%H:%M:%S)" user alice)" | python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])')
echo "thread $THREAD"

say "1. Only placeholder keys"
exec_in "$THREAD" 'env | grep -E "KEY|TOKEN"'

say "2. Nothing else is reachable"
exec_in "$THREAD" "curl -sS -m 8 -w ' [%{http_code}]' https://example.com; curl -sS -m 8 -w ' [%{http_code}]' http://$INTERNAL:8080/orders"

[ "${WITH_MODEL:-1}" = 1 ] || { echo; echo "WITH_MODEL=0: skipping the agent moments"; exit 0; }

say "2b. The same data through the connector, asked of the agent"
post "/api/threads/$THREAD/messages" "$(json user alice text 'List the orders.')" >/dev/null
wait_for "$THREAD" '"event":{"type":"result"' 120
last_result "$THREAD"

say "3. A write waits for approval"
post "/api/threads/$THREAD/messages" "$(json user alice text 'Refund order 1003.')" >/dev/null
wait_for "$THREAD" '"pending":[{' 120
echo "(the UI shows Approve / Deny for this held write; approving through the API)"
APPROVAL=$(events "$THREAD" 1 | grep '"pending":\[{' | tail -1 | sed -E 's/^data: //' | python3 -c 'import json,sys; p=json.load(sys.stdin)["pending"][0]; print(p["id"]); print(p["method"], p["path"], p["body"], file=sys.stderr)')
post "/api/threads/$THREAD/approvals/$APPROVAL" "{\"approve\":true,\"by\":\"alice\"}"

say "4. Switch the user mid-thread"
sleep 20
post "/api/threads/$THREAD/messages" "$(json user bob text 'Cancel order 1004.')" >/dev/null
sleep 40
events "$THREAD" 1 | grep -E 'user switched|bob' | tail -4

say "5. Idle, pause, resume with the agent process alive"
post "/api/threads/$THREAD/messages" "$(json user alice text 'Start `orders watch` in the background and tell me its pid.')" >/dev/null
sleep 40
echo "waiting $IDLE_WAIT s for the thread to pause"; sleep "$IDLE_WAIT"
curl -fsS "$CP/api/threads" | python3 -c 'import json,sys; [print(t["id"], t["state"]) for t in json.load(sys.stdin)["threads"]]'
post "/api/threads/$THREAD/messages" "$(json user alice text 'Is the orders watch still running? Check with pgrep and give the pid.')" >/dev/null
sleep 40
last_result "$THREAD"
