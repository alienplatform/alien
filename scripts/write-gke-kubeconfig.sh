#!/usr/bin/env bash
# Write a GKE kubeconfig that authenticates with a target service account.
#
# Usage:
#   ./scripts/write-gke-kubeconfig.sh \
#     --key-file /tmp/target-sa.json \
#     --project "$GOOGLE_TARGET_PROJECT_ID" \
#     --cluster "$ALIEN_TEST_GKE_CLUSTER_NAME" \
#     --location "$ALIEN_TEST_GKE_CLUSTER_LOCATION" \
#     --kubeconfig "$ALIEN_TEST_GKE_KUBECONFIG"
#
# Requires gcloud with the gke-gcloud-auth-plugin installed.
set -euo pipefail

key_file=""
project=""
cluster=""
location=""
kubeconfig=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --key-file)
      key_file="${2:?--key-file requires a path}"
      shift 2
      ;;
    --project)
      project="${2:?--project requires a project ID}"
      shift 2
      ;;
    --cluster)
      cluster="${2:?--cluster requires a cluster name}"
      shift 2
      ;;
    --location)
      location="${2:?--location requires a cluster location}"
      shift 2
      ;;
    --kubeconfig)
      kubeconfig="${2:?--kubeconfig requires a path}"
      shift 2
      ;;
    *)
      echo "unknown argument: $1" >&2
      exit 2
      ;;
  esac
done

for value_name in key_file project cluster location kubeconfig; do
  if [[ -z "${!value_name}" ]]; then
    echo "missing required argument: ${value_name}" >&2
    exit 2
  fi
done

mkdir -p "$(dirname "$kubeconfig")"
gcloud auth activate-service-account --key-file="$key_file" --quiet
rm -f "$kubeconfig"
KUBECONFIG="$kubeconfig" gcloud container clusters get-credentials "$cluster" \
  --location "$location" \
  --project "$project" \
  --quiet
chmod 600 "$kubeconfig"
