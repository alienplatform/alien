#!/usr/bin/env bash
# Local stand-in for your customers' environments.
#
#   ./local/cluster.sh up     create everything below (safe to run again)
#   ./local/cluster.sh down   delete it
#
# `up` creates:
# - a kind cluster named alien-demo (kubectl context kind-alien-demo) that
#   plays every customer, one namespace each
# - S3-compatible storage in it (namespace storage) with one bucket per
#   customer: customer-1-files, customer-2-files, customer-3-files
# - site-registry, a plain registry that plays the air-gapped customer's own
#   registry, at localhost:5001
#
# The nodes are allowed to pull over plain HTTP from your manager
# (alien-manager:8080) and from site-registry. Real clusters pull from
# registries that serve HTTPS, so they need none of this.
set -euo pipefail
cd "$(dirname "$0")"

CLUSTER=alien-demo
CONTEXT="kind-$CLUSTER"

# Let every node pull from `host` over plain HTTP at `endpoint`.
trust_registry() {
  local node=$1 host=$2 endpoint=$3
  docker exec "$node" mkdir -p "/etc/containerd/certs.d/$host"
  printf '[host."%s"]\n  capabilities = ["pull", "resolve"]\n' "$endpoint" |
    docker exec -i "$node" tee "/etc/containerd/certs.d/$host/hosts.toml" >/dev/null
}

case "${1:-}" in
  up)
    if ! kind get clusters 2>/dev/null | grep -qx "$CLUSTER"; then
      kind create cluster --name "$CLUSTER" --config kind.yaml --wait 120s
    fi
    for node in $(kind get nodes --name "$CLUSTER"); do
      trust_registry "$node" alien-manager:8080 http://alien-manager:8080
      trust_registry "$node" localhost:5001 http://site-registry:5000
    done

    if ! docker inspect site-registry >/dev/null 2>&1; then
      docker run -d --name site-registry --network kind -p 127.0.0.1:5001:5000 registry:2 >/dev/null
    fi

    kubectl --context "$CONTEXT" apply -f storage.yaml
    kubectl --context "$CONTEXT" -n storage rollout status deploy/s3 --timeout=300s
    kubectl --context "$CONTEXT" -n storage wait --for=condition=complete job/buckets --timeout=300s
    echo
    echo "Ready: cluster $CONTEXT, storage at http://s3.storage.svc, site registry at localhost:5001."
    ;;
  down)
    kind delete cluster --name "$CLUSTER"
    docker rm -f site-registry >/dev/null 2>&1 || true
    ;;
  *)
    echo "usage: $0 up|down" >&2
    exit 1
    ;;
esac
