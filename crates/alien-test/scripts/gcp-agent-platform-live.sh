#!/usr/bin/env bash
# Live checks for the GCP Agent Platform sandbox, run by hand against a test project.
#
#   image     Build alien-sandbox-agent (plus git), push it to an Artifact Registry repo in the
#             project, and grant the project's Agent Sandbox service agent Reader on that repo.
#   suite     Run tests/gcp_agent_platform_sandbox_live.rs against that image.
#   iam-check Prove a role bound on one reasoning engine through its IAM policy (what
#             `google_vertex_ai_reasoning_engine_iam_member` writes) authorizes template verbs
#             under that engine and nothing else: the holder can create and replace a template on
#             its engine, cannot create or delete an engine, and cannot touch another engine's
#             templates.
#   teardown  Delete every engine, service account and role this script created.
#
# Required: GOOGLE_TARGET_PROJECT_ID, GOOGLE_TARGET_REGION, and gcloud logged in as a principal
# that can administer the project. `suite` also needs GOOGLE_TARGET_SERVICE_ACCOUNT_KEY,
# ALIEN_TEST_GIT_TOKEN and ALIEN_TEST_PRIVATE_REPO (see the suite's module docs).
set -euo pipefail

project="${GOOGLE_TARGET_PROJECT_ID:?set GOOGLE_TARGET_PROJECT_ID}"
region="${GOOGLE_TARGET_REGION:?set GOOGLE_TARGET_REGION}"
repo="${ALIEN_TEST_GCP_AGENT_REPO:-alien-sandbox-live}"
tag="${ALIEN_TEST_GCP_AGENT_TAG:-live-$(git rev-parse --short HEAD)}"
image="${region}-docker.pkg.dev/${project}/${repo}/alien-sandbox-agent:${tag}"
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
state_dir="${ALIEN_TEST_GCP_STATE_DIR:-${TMPDIR:-/tmp}/alien-gcp-agent-platform-live}"
host="https://${region}-aiplatform.googleapis.com/v1"
parent="projects/${project}/locations/${region}"
holder_id="alien-sbx-templates-probe"
holder="${holder_id}@${project}.iam.gserviceaccount.com"
# A deleted custom role lingers for 7 days, still describable, and its id cannot be reused, so each
# run creates its own and records it for teardown.
role_prefix="alienSandboxTemplatesProbe"
mkdir -p "$state_dir"

project_number() {
  gcloud projects describe "$project" --format='value(projectNumber)'
}

# Calls the regional API as `$1` (an access token) and prints the HTTP status, keeping the body in
# $state_dir/last.json.
call() {
  local token="$1" method="$2" path="$3" body="${4:-}"
  curl -sS -o "$state_dir/last.json" -w '%{http_code}' -X "$method" \
    -H "Authorization: Bearer ${token}" -H 'Content-Type: application/json' \
    ${body:+--data "$body"} "${host}/${path}"
}

# Polls a long-running operation to completion and prints the created resource name.
await_operation() {
  local token="$1" operation="$2"
  for _ in $(seq 1 150); do
    call "$token" GET "$operation" >/dev/null
    if [[ "$(jq -r '.done // false' "$state_dir/last.json")" == "true" ]]; then
      jq -e '.error == null' "$state_dir/last.json" >/dev/null || {
        cat "$state_dir/last.json" >&2
        return 1
      }
      jq -r '.response.name' "$state_dir/last.json"
      return 0
    fi
    sleep 4
  done
  echo "operation ${operation} did not finish" >&2
  return 1
}

create_engine() {
  local token="$1" status operation
  status="$(call "$token" POST "${parent}/reasoningEngines" \
    "{\"displayName\":\"alien-sbx-live-iam-$(date +%s)\"}")"
  [[ "$status" == 200 ]] || { echo "engine create returned $status" >&2; cat "$state_dir/last.json" >&2; return 1; }
  operation="$(jq -r '.name' "$state_dir/last.json")"
  await_operation "$token" "$operation"
}

template_body() {
  jq -nc --arg image "$image" --arg name "alien-sbx-live-iam-$(uuidgen | tr -d - | tr '[:upper:]' '[:lower:]' | cut -c1-12)" '{
    displayName: $name,
    customContainerEnvironment: {
      customContainerSpec: {imageUri: $image},
      resources: {limits: {cpu: "2", memory: "4Gi"}}
    },
    egressControlConfig: {internetAccess: false}
  }'
}

expect_status() {
  local want="$1" got="$2" what="$3"
  if [[ "$got" == "$want" ]]; then
    echo "PASS  ${what} -> ${got}"
  else
    echo "FAIL  ${what} -> ${got}, expected ${want}" >&2
    cat "$state_dir/last.json" >&2
    return 1
  fi
}

# The gcloud caller as an IAM member: a service account needs `serviceAccount:`, a person `user:`.
caller_member() {
  local account
  account="$(gcloud config get-value account 2>/dev/null)"
  if [[ "$account" == *.gserviceaccount.com ]]; then
    echo "serviceAccount:${account}"
  else
    echo "user:${account}"
  fi
}

cmd_image() {
  gcloud services enable artifactregistry.googleapis.com aiplatform.googleapis.com --project "$project"
  gcloud artifacts repositories describe "$repo" --location "$region" --project "$project" >/dev/null 2>&1 \
    || gcloud artifacts repositories create "$repo" --repository-format docker \
      --location "$region" --project "$project"

  # The Dockerfile copies both musl binaries; Agent Platform runs amd64.
  (cd "$repo_root" && cargo zigbuild --release -p alien-sandbox-agent --target x86_64-unknown-linux-musl \
    && cargo zigbuild --release -p alien-sandbox-agent --target aarch64-unknown-linux-musl)
  gcloud auth configure-docker "${region}-docker.pkg.dev" --quiet
  docker buildx build --platform linux/amd64 -f "$repo_root/docker/Dockerfile.alien-sandbox-agent" \
    -t "$image" --push "$repo_root"

  # Created on first use of the Agent Sandbox API; `services identity create` forces it now.
  gcloud beta services identity create --service aiplatform.googleapis.com --project "$project" >/dev/null
  gcloud artifacts repositories add-iam-policy-binding "$repo" --location "$region" --project "$project" \
    --member "serviceAccount:service-$(project_number)@gcp-sa-vertex-sandbox.iam.gserviceaccount.com" \
    --role roles/artifactregistry.reader
  echo "ALIEN_TEST_GCP_AGENT_IMAGE=${image}"
}

cmd_suite() {
  : "${GOOGLE_TARGET_SERVICE_ACCOUNT_KEY:?set GOOGLE_TARGET_SERVICE_ACCOUNT_KEY}"
  : "${ALIEN_TEST_GIT_TOKEN:?set ALIEN_TEST_GIT_TOKEN}"
  : "${ALIEN_TEST_PRIVATE_REPO:?set ALIEN_TEST_PRIVATE_REPO}"
  cd "$repo_root"
  ALIEN_TEST_GCP_AGENT_IMAGE="${ALIEN_TEST_GCP_AGENT_IMAGE:-$image}" \
    cargo test -p alien-test --test gcp_agent_platform_sandbox_live -- --ignored --test-threads=1
}

cmd_iam_check() {
  local admin_token own other role_name holder_token status operation template
  admin_token="$(gcloud auth print-access-token)"

  own="$(create_engine "$admin_token")"
  other="$(create_engine "$admin_token")"
  printf '%s\n%s\n' "$own" "$other" >"$state_dir/engines"
  echo "own engine:   $own"
  echo "other engine: $other"

  # The same verbs as permission-sets/sandbox/templates.jsonc, as a project custom role that is
  # bound only on the engine, never on the project.
  local role_id
  role_id="${role_prefix}_$(date +%s)"
  echo "$role_id" >>"$state_dir/roles"
  gcloud iam roles create "$role_id" --project "$project" --title "Alien sandbox templates probe" \
      --permissions aiplatform.sandboxEnvironmentTemplates.create,aiplatform.sandboxEnvironmentTemplates.delete,aiplatform.sandboxEnvironmentTemplates.get,aiplatform.sandboxEnvironmentTemplates.list
  role_name="projects/${project}/roles/${role_id}"
  gcloud iam service-accounts describe "$holder" --project "$project" >/dev/null 2>&1 \
    || gcloud iam service-accounts create "$holder_id" --project "$project"
  # The holder is impersonated rather than keyed, so the caller needs Token Creator on it.
  gcloud iam service-accounts add-iam-policy-binding "$holder" --project "$project" \
    --member "$(caller_member)" --role roles/iam.serviceAccountTokenCreator >/dev/null

  # Engine IAM is served on v1beta1, the API version the google-beta IAM member resource calls.
  status="$(host="https://${region}-aiplatform.googleapis.com/v1beta1" call "$admin_token" POST "${own}:setIamPolicy" \
    "{\"policy\":{\"bindings\":[{\"role\":\"${role_name}\",\"members\":[\"serviceAccount:${holder}\"]}]}}")"
  expect_status 200 "$status" "bind the role on the own engine"

  echo "waiting 90s for IAM propagation"
  sleep 90
  holder_token="$(gcloud auth print-access-token --impersonate-service-account "$holder")"

  status="$(call "$holder_token" POST "${own}/sandboxEnvironmentTemplates" "$(template_body)")"
  expect_status 200 "$status" "create a template on the own engine"
  operation="$(jq -r '.name' "$state_dir/last.json")"
  template="$(await_operation "$holder_token" "$operation")"

  status="$(call "$holder_token" POST "${own}/sandboxEnvironmentTemplates" "$(template_body)")"
  expect_status 200 "$status" "create a replacement template on the own engine"
  await_operation "$holder_token" "$(jq -r '.name' "$state_dir/last.json")" >/dev/null
  status="$(call "$holder_token" DELETE "$template")"
  expect_status 200 "$status" "delete the replaced template on the own engine"

  status="$(call "$holder_token" POST "${parent}/reasoningEngines" '{"displayName":"alien-sbx-live-iam-denied"}')"
  expect_status 403 "$status" "create an engine"
  status="$(call "$holder_token" DELETE "$own")"
  expect_status 403 "$status" "delete the own engine"

  status="$(call "$holder_token" POST "${other}/sandboxEnvironmentTemplates" "$(template_body)")"
  expect_status 403 "$status" "create a template on another engine"
  status="$(call "$holder_token" GET "${other}/sandboxEnvironmentTemplates")"
  expect_status 403 "$status" "list templates on another engine"
  status="$(call "$admin_token" POST "${other}/sandboxEnvironmentTemplates" "$(template_body)")"
  expect_status 200 "$status" "admin creates a template on another engine"
  local other_template
  other_template="$(await_operation "$admin_token" "$(jq -r '.name' "$state_dir/last.json")")"
  status="$(call "$holder_token" GET "$other_template")"
  expect_status 403 "$status" "get another engine's template"
  status="$(call "$holder_token" DELETE "$other_template")"
  expect_status 403 "$status" "delete another engine's template"

  # Sessions are sandbox contents: template verbs must not reach them, even on the own engine.
  status="$(call "$holder_token" POST "${own}/sandboxEnvironments" '{}')"
  expect_status 403 "$status" "create a session on the own engine"
  status="$(call "$holder_token" GET "${own}/sandboxEnvironments")"
  expect_status 403 "$status" "list sessions on the own engine"
}

cmd_teardown() {
  local admin_token engine
  admin_token="$(gcloud auth print-access-token)"
  if [[ -f "$state_dir/engines" ]]; then
    while read -r engine; do
      [[ -n "$engine" ]] || continue
      echo "delete $engine -> $(call "$admin_token" DELETE "${engine}?force=true")"
    done <"$state_dir/engines"
    rm -f "$state_dir/engines"
  fi
  gcloud iam service-accounts delete "$holder" --project "$project" --quiet 2>/dev/null || true
  if [[ -f "$state_dir/roles" ]]; then
    while read -r role; do
      [[ -n "$role" ]] || continue
      gcloud iam roles delete "$role" --project "$project" --quiet 2>/dev/null || true
    done <"$state_dir/roles"
    rm -f "$state_dir/roles"
  fi
}

case "${1:-}" in
  image) cmd_image ;;
  suite) cmd_suite ;;
  iam-check) cmd_iam_check ;;
  teardown) cmd_teardown ;;
  *) echo "usage: $0 image|suite|iam-check|teardown" >&2; exit 2 ;;
esac
