#!/usr/bin/env bash
# Release signing on Kubernetes, in a disposable kind cluster: the chart's signing ConfigMaps
# survive `helm uninstall`, the Operator can't rewrite its settings (R14), values can't pick
# another Operator build (R12), and air-gapped sync refuses a foreign chart on a protected
# site (R13). The product chart's Remote Operator can't rewrite the Operator. Every attack also
# runs unprotected, where it succeeds.
#
# A local registry serves the fixture Operator, which protected charts pull by digest.
#
# Usage: scripts/helm-signing-kind/run.sh
#   KIND_CLUSTER   cluster name (default helm-signing-$$); deleted on exit
#   REGISTRY_PORT  host port of the local registry (default 5001); removed on exit
#   OUT            keep the test output here
set -euo pipefail

repo=$(git -C "$(dirname "$0")" rev-parse --show-toplevel)
cluster=${KIND_CLUSTER:-helm-signing-$$}
registry_port=${REGISTRY_PORT:-5001}
registry="$cluster-registry"
work=$(mktemp -d)
out=${OUT:-$work}
kubeconfig="$work/kubeconfig"

cleanup() {
  kind delete cluster --name "$cluster" >/dev/null 2>&1 || true
  docker rm -f "$registry" >/dev/null 2>&1 || true
  rm -rf "$work"
}
trap cleanup EXIT

docker run -d --restart=no -p "127.0.0.1:$registry_port:5000" --name "$registry" registry:2
cat > "$work/kind.yaml" <<'KIND'
kind: Cluster
apiVersion: kind.x-k8s.io/v1alpha4
containerdConfigPatches:
  - |-
    [plugins."io.containerd.grpc.v1.cri".registry]
      config_path = "/etc/containerd/certs.d"
KIND
kind create cluster --name "$cluster" --kubeconfig "$kubeconfig" --config "$work/kind.yaml" --wait 120s
docker network connect kind "$registry"
# Nodes reach the registry by its container name; the host pushes to localhost.
for node in $(kind get nodes --name "$cluster"); do
  docker exec "$node" mkdir -p "/etc/containerd/certs.d/localhost:$registry_port"
  printf '[host."http://%s:5000"]\n' "$registry" |
    docker exec -i "$node" cp /dev/stdin "/etc/containerd/certs.d/localhost:$registry_port/hosts.toml"
done

cd "$repo"
KUBECONFIG="$kubeconfig" ALIEN_HELM_TEST_KUBECONFIG="$kubeconfig" \
  ALIEN_HELM_TEST_REGISTRY="localhost:$registry_port" \
  cargo test -p alien-helm --test signing_kind -- --ignored --test-threads=1 \
  2>&1 | tee "$out/alien-helm.log"
KUBECONFIG="$kubeconfig" ALIEN_SYNC_TEST_CONTEXT="kind-$cluster" \
  cargo test -p alien-deploy-cli --lib -- --ignored --exact --test-threads=1 \
  commands::sync::tests::protected_site_refuses_a_foreign_chart_from_chart_defaults \
  commands::sync::tests::bundle_strings_cannot_assign_other_values \
  2>&1 | tee "$out/alien-deploy-cli.log"
