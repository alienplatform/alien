#!/usr/bin/env bash
# Helm setup update e2e on kind. See README.md.
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../../.." && pwd)
MANAGER_BIN=${ALIEN_MANAGER_BIN:-$ROOT/target/debug/alien-manager}
OPERATOR_BIN=${ALIEN_OPERATOR_BIN:-$ROOT/target/debug/alien-operator}
CLUSTER=${KIND_CLUSTER:-helm-setup-update}
CTX=kind-$CLUSTER
W=$(mktemp -d)
NS=alien
PORT=8080
GW=$(docker network inspect kind -f '{{(index .IPAM.Config 0).Gateway}}' 2>/dev/null || echo 172.18.0.1)
M=http://localhost:$PORT
REG=$GW:$PORT
export HELM_EXPERIMENTAL_OCI=1
K="kubectl --context $CTX -n $NS"
echo "work directory: $W"

CREATED=
cleanup() {
  [ "${KEEP:-}" = 1 ] && return
  [ -z "${MANAGER_PID:-}" ] || kill "$MANAGER_PID" 2>/dev/null || true
  # Only delete a cluster this run created.
  [ -z "$CREATED" ] || kind delete cluster --name "$CLUSTER" >/dev/null 2>&1 || true
}
trap cleanup EXIT
# A failure inside `$(...)` (a chart that cannot be built) must stop the run,
# not just its subshell: it signals the script, whose TERM trap exits.
trap 'exit 1' TERM
fail() {
  { echo "FAIL: $*"; echo "--- manager log tail"; tail -30 "$W/manager.log"; } >&2
  [ "$BASHPID" = "$$" ] || kill -TERM $$
  exit 1
}

# --- cluster, operator image, manager -------------------------------------
if kind get clusters | grep -qx "$CLUSTER"; then
  echo "cluster $CLUSTER already exists; set KIND_CLUSTER to a new name" >&2
  exit 1
fi
kind create cluster --name "$CLUSTER" >/dev/null
CREATED=1
mkdir -p "$W/img" && cp "$OPERATOR_BIN" "$W/img/alien-operator"
# A debug build's symbols do not fit the chart's 512Mi memory limit.
strip "$W/img/alien-operator"
cat > "$W/img/Dockerfile" <<'D'
FROM ubuntu:24.04
RUN apt-get update -qq && apt-get install -y -qq ca-certificates && rm -rf /var/lib/apt/lists/* && mkdir -p /var/lib/operator && chown 1000:1000 /var/lib/operator
COPY alien-operator /app/alien-operator
USER 1000
ENTRYPOINT ["/app/alien-operator"]
D
docker build -q -t alien-operator:e2e "$W/img" >/dev/null
kind load docker-image alien-operator:e2e --name "$CLUSTER" >/dev/null
mkdir -p "$W/data"
cat > "$W/manager.toml" <<T
[server]
port = $PORT
host = "0.0.0.0"
base-url = "http://$REG"
deployment-interval-secs = 3
[database]
state-dir = "$W/data"
[operator]
image = "alien-operator:e2e"
T
(cd "$W" && exec "$MANAGER_BIN" -c manager.toml > manager.log 2>&1) &
MANAGER_PID=$!
for _ in $(seq 30); do curl -sf $M/health >/dev/null && break; sleep 1; done
TOKEN=$(grep -o 'ax_admin_[0-9a-f]*' "$W/manager.log" | head -1)
[ -n "$TOKEN" ] || fail "manager did not print an admin token"

api() { curl -sf -H "Authorization: Bearer $TOKEN" -H 'content-type: application/json' "$@"; }
json() { python3 -c "import sys,json;d=json.load(sys.stdin);print($1)"; }

# --- releases: frozen service accounts, plus one frozen storage -------------
stack() { # stack <comma-separated service account ids> [storage]
  python3 - "$1" "${2:-}" <<'P'
import json, sys
res = {}
for i in filter(None, sys.argv[1].split(",")):
    res[i] = {"config": {"id": i, "stackPermissionSets": [], "type": "service-account"},
              "dependencies": [], "lifecycle": "frozen", "remoteAccess": False}
if sys.argv[2]:
    res[sys.argv[2]] = {"config": {"corsAllowedOrigins": [], "id": sys.argv[2], "lifecycleRules": [],
              "publicRead": False, "type": "storage", "versioning": False},
              "dependencies": [], "lifecycle": "frozen", "remoteAccess": False}
print(json.dumps({"id": "app", "permissions": {"management": "auto", "profiles": {}}, "resources": res}))
P
}
release() { # release <service accounts> [storage] -> release id
  python3 -c "import json,sys;print(json.dumps({'stack':{'kubernetes':json.loads(sys.argv[1])},'projectId':'default'}))" "$(stack "$@")" \
    | api -XPOST $M/v1/releases -d @- | json "d['id']"
}

# --- charts: the manager's chart for a release, as it serves it ----------------
# The setup the chart carries (release, target, fingerprint) is the manager's
# own. Only TARGET rewrites the setup target, to stand for a chart from another
# install channel.
chart() { # chart <n> <release id> <service accounts> -> path of the packaged chart
  local tag version d
  for tag in $(curl -sf -u "x:$DGT" "$M/v2/charts/app/tags/list" | json "' '.join(d['tags'])"); do
    [ "$(curl -sf -u "x:$DGT" "$M/v2/charts/app/manifests/$tag" | json "d['annotations']['org.opencontainers.image.revision']")" = "$2" ] && version=$tag
  done
  [ -n "${version:-}" ] || fail "no chart is served for release $2"
  d=$W/charts/$1-${TARGET:-served}; rm -rf "$d"; mkdir -p "$d"
  helm pull oci://$REG/charts/app --version "$version" --plain-http -d "$d" --untar >/dev/null
  grep -q "setup.alien.dev/release-id: \"$2\"" "$d/app/Chart.yaml" || fail "the chart for release $2 carries no setup"
  python3 - "$d/app/values.yaml" "$3" <<'P'
import sys
p, ids = sys.argv[1], [i for i in sys.argv[2].split(",") if i]
s = open(p).read()
s = s.replace("serviceAccounts:\n  {}", "serviceAccounts:\n" + "".join(f"  {i}: {{}}\n" for i in ids), 1)
open(p, "w").write(s)
P
  if [ -n "${TARGET:-}" ]; then
    sed "s|setup.alien.dev/target: .*|setup.alien.dev/target: \"$TARGET\"|" "$d/app/Chart.yaml" > "$d/Chart.yaml.new" && mv "$d/Chart.yaml.new" "$d/app/Chart.yaml"
  fi
  # Package it under the case number, whatever version the manager gave it.
  sed "s/^version: .*/version: 1.0.$1/" "$d/app/Chart.yaml" > "$d/Chart.yaml.new" && mv "$d/Chart.yaml.new" "$d/app/Chart.yaml"
  helm package "$d/app" -d "$d" >/dev/null; echo "$d/app-1.0.$1.tgz"
}
upgrade() { helm --kube-context $CTX upgrade app "$1" -n $NS --set management.token="$DGT" --timeout 120s "${@:2}" >/dev/null; }

dep() { api $M/v1/deployments | python3 -c "import sys,json;print(json.dumps(json.load(sys.stdin)['items'][0]))"; }
field() { dep | python3 -c "import sys,json;d=json.load(sys.stdin);m=d['runtimeMetadata'];print($1)"; }
eventually() { # until <description> <python expression over d and m> [seconds]
  local n=$(( ${3:-180} / 3 ))
  for _ in $(seq "$n"); do [ "$(field "$2")" = True ] && { echo "PASS: $1"; return; }; sleep 3; done
  fail "$1 (state: $(field "d['status'],d.get('currentReleaseId'),d.get('desiredReleaseId'),m.get('setupReportRefusal')"))"
}
holds() { # holds <description> <expression>: the condition stays true for 30 seconds
  for _ in $(seq 10); do [ "$(field "$2")" = True ] || fail "$1"; sleep 3; done; echo "PASS: $1"
}
revision() { helm --kube-context $CTX history app -n $NS -o json | python3 -c "import sys,json;print(max(r['revision'] for r in json.load(sys.stdin)))"; }
operator_fingerprint() { # the setup fingerprint the running Operator was started with
  $K get deploy -o json | python3 -c "
import sys,json
for d in json.load(sys.stdin)['items']:
    for c in d['spec']['template']['spec']['containers']:
        for e in c.get('env', []):
            if e['name'] == 'OPERATOR_SETUP_FINGERPRINT': print(e['value'])"
}
chart_fingerprint() { tar -xzOf "$1" app/Chart.yaml | sed -n 's|.*setup.alien.dev/fingerprint: "\(.*\)"|\1|p'; }
mints() { grep -c "Chart completes the setup update" "$W/manager.log" || true; }

# --- install release 1 ----------------------------------------------------------
R1=$(release execution)
DG=$(api -XPOST $M/v1/deployment-groups -d '{"name":"e2e"}' | json "d['id']")
DGT=$(api -XPOST $M/v1/deployment-groups/$DG/tokens -d '{}' | json "d['token']")
echo "$DGT" | helm registry login $REG --plain-http -u x --password-stdin >/dev/null
helm --kube-context $CTX install app "$(chart 1 "$R1" execution)" -n $NS --create-namespace \
  --set management.token="$DGT" --wait --timeout 180s >/dev/null
eventually "release 1 installed and running" "d['status']=='running' and d.get('currentReleaseId')=='$R1'"

# --- case: chart for an older release than the one waiting is refused --------------
R2=$(release execution,reader)
eventually "deployment pauses on a release it cannot apply" "d['status']=='update-failed' and d.get('desiredReleaseId')=='$R2'"
# --- case: a retry without a chart upgrade does not complete the update -------------
DEP=$(dep | json "d['id']")
api -XPOST $M/v1/deployments/$DEP/retry >/dev/null
holds "a retry without the upgrade leaves the update waiting" "d.get('currentReleaseId')=='$R1'"
eventually "the retried update pauses again" "d['status']=='update-failed' and d.get('desiredReleaseId')=='$R2'"
[ "$(mints)" = 0 ] || fail "a retry minted a grant"

R3=$(release execution,reader,writer)
eventually "waiting release moves to release 3" "d.get('desiredReleaseId')=='$R3'"
upgrade "$(chart 2 "$R2" execution,reader)"
eventually "stale chart refused, naming the release to use" \
  "'waiting for release $R3' in ((m.get('setupReportRefusal') or {}).get('reason',''))"
[ "$(mints)" = 0 ] || fail "a stale chart minted a grant"

# --- case: a release adding a cloud resource is refused ------------------------------
R4=$(release execution archive)
eventually "waiting for release 4" "d.get('desiredReleaseId')=='$R4'"
upgrade "$(chart 4 "$R4" execution)"
eventually "cloud resource change refused with the cloud setup message" \
  "'lives in your cloud account' in ((m.get('setupReportRefusal') or {}).get('reason',''))"
eventually "the waiting release is recorded as needing the cloud setup" \
  "(m.get('cloudSetupRequired') or {}).get('releaseId')=='$R4'"
holds "that verdict stays while the release waits" "(m.get('cloudSetupRequired') or {}).get('releaseId')=='$R4'"

# --- case: the chart for the waiting release completes the update --------------------
R5=$(release execution,reader)
eventually "waiting for release 5" "d.get('desiredReleaseId')=='$R5'"
eventually "the verdict for release 4 is cleared once release 5 waits" "not m.get('cloudSetupRequired')"
BEFORE_R5=$(revision)
upgrade "$(chart 5 "$R5" execution,reader)"
eventually "helm upgrade completes the setup update" \
  "d['status']=='running' and d.get('currentReleaseId')=='$R5'"
$K get sa app-reader-sa >/dev/null || fail "the new service account was not created"

# --- case: a chart-only bump mints nothing ----------------------------------------------
M0=$(mints); POD=$($K get pods -l app.kubernetes.io/name=app -o name | head -1)
B=$W/charts/bump; rm -rf "$B"; mkdir -p "$B"; cp -r "$W/charts/5-served/app" "$B/app"
sed 's/^version: 1.0.5/version: 1.0.5-1/' "$B/app/Chart.yaml" > "$B/Chart.yaml.new" && mv "$B/Chart.yaml.new" "$B/app/Chart.yaml"; helm package "$B/app" -d "$B" >/dev/null
upgrade "$B/app-1.0.5-1.tgz" --wait
sleep 20
[ "$(mints)" = "$M0" ] || fail "a chart-only bump minted a grant"
echo "PASS: chart-only bump mints nothing"

# --- case: rollback after success mints nothing -------------------------------------------
helm --kube-context $CTX rollback app "$BEFORE_R5" -n $NS --wait >/dev/null
holds "rollback after success keeps the deployment running" "d['status']=='running'"
[ "$(mints)" = "$M0" ] || fail "a rollback minted a grant"

# --- case: a chart for another install channel is refused ------------------------------------
R6=$(release execution,reader,writer)
eventually "waiting for release 6" "d.get('desiredReleaseId')=='$R6'"
# The chart installed now (release 4's, after the rollback above) carries a
# different setup than release 6; the rollback case below returns to it.
BEFORE_R6=$(revision)
upgrade "$(TARGET=other chart 6 "$R6" execution,reader,writer)"
eventually "chart from another install channel refused" \
  "'different setup' in ((m.get('setupReportRefusal') or {}).get('reason',''))"

# --- case: rolling back to a chart with another setup after the grant leaves the update waiting ---
# The Operator keeps a grant only for the fingerprint it was started with, so
# the rollback must reach a chart whose setup differs from release 6's. The
# other-channel chart above is not one: it carries release 6's fingerprint.
P6=$(chart 6 "$R6" execution,reader,writer)
upgrade "$P6" &
UPGRADE_PID=$!
GRANTED=
for _ in $(seq 120); do [ "$(field "bool(m.get('setupUpdateAuthorization'))")" = True ] && { GRANTED=1; break; }; sleep 1; done
[ -n "$GRANTED" ] || fail "no grant was issued for the rollback case"
wait "$UPGRADE_PID" || true
helm --kube-context $CTX rollback app "$BEFORE_R6" -n $NS --wait >/dev/null
ROLLED_FP=$(operator_fingerprint); P6_FP=$(chart_fingerprint "$P6")
[ -n "$ROLLED_FP" ] && [ -n "$P6_FP" ] && [ "$ROLLED_FP" != "$P6_FP" ] \
  || fail "the rollback did not reach a chart with another setup ('$ROLLED_FP' vs '$P6_FP')"
holds "the grant does not apply to the rolled-back chart" "d['status']!='running' or d.get('currentReleaseId')!='$R6'"
upgrade "$P6"
eventually "upgrading to the right chart completes the update" "d['status']=='running' and d.get('currentReleaseId')=='$R6'"

# --- case: the older-Helm fallback, run as the dashboard shows it ------------------------------------
R7=$(release execution,reader,writer,auditor)
eventually "waiting for release 7" "d.get('desiredReleaseId')=='$R7'"
helm --kube-context $CTX get values app -n $NS -o yaml > "$W/current.yaml"
helm --kube-context $CTX upgrade app "$(chart 7 "$R7" execution,reader,writer,auditor)" -n $NS -f "$W/current.yaml" --timeout 120s >/dev/null
rm -f "$W/current.yaml"
eventually "the fallback (-o yaml values file) completes the update" "d['status']=='running' and d.get('currentReleaseId')=='$R7'"

# --- case: helm template | kubectl apply ---------------------------------------------------------
R8=$(release execution,reader,writer,auditor,billing)
eventually "waiting for release 8" "d.get('desiredReleaseId')=='$R8'"
KEY=$($K get secret app -o jsonpath='{.data.encryption-key}' | base64 -d)
helm template app "$(chart 8 "$R8" execution,reader,writer,auditor,billing)" -n $NS --no-hooks \
  --set management.token="$DGT" --set runtime.encryption.key="$KEY" | $K apply -f - >/dev/null
eventually "helm template | kubectl apply completes the update" "d['status']=='running' and d.get('currentReleaseId')=='$R8'"

echo "ALL PASS"
