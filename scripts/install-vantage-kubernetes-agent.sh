#!/usr/bin/env bash
# Install or update the Vantage Kubernetes Agent on persistent shared E2E clusters.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

env_file=".env.test"
install_eks=false
install_aks=false
chart_version="1.8.2"

usage() {
  cat <<'EOF'
Usage:
  scripts/install-vantage-kubernetes-agent.sh [--env-file .env.test] [--eks] [--aks] [--all]

Requires:
  VANTAGE_KUBERNETES_AGENT_TOKEN
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --env-file)
      env_file="${2:?--env-file requires a path}"
      shift 2
      ;;
    --eks)
      install_eks=true
      shift
      ;;
    --aks)
      install_aks=true
      shift
      ;;
    --all)
      install_eks=true
      install_aks=true
      shift
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if [[ "$install_eks" != "true" && "$install_aks" != "true" ]]; then
  install_eks=true
  install_aks=true
fi

if [[ ! -f "$env_file" ]]; then
  echo "env file not found: $env_file" >&2
  exit 1
fi

if [[ -z "${VANTAGE_KUBERNETES_AGENT_TOKEN:-}" ]]; then
  echo "VANTAGE_KUBERNETES_AGENT_TOKEN is required" >&2
  exit 1
fi

set -a
# shellcheck disable=SC1090
source "$env_file"
set +a

helm repo add vantage https://vantage-sh.github.io/helm-charts >/dev/null
helm repo update vantage >/dev/null

ensure_secret() {
  local kubeconfig="$1"
  local before
  local after

  kubectl --kubeconfig "$kubeconfig" create namespace vantage --dry-run=client -o yaml | kubectl --kubeconfig "$kubeconfig" apply -f -
  before="$(kubectl --kubeconfig "$kubeconfig" get secret vantage-api-token -n vantage -o jsonpath='{.data.token}' 2>/dev/null || true)"
  kubectl --kubeconfig "$kubeconfig" create secret generic vantage-api-token \
    --namespace vantage \
    --from-literal=token="$VANTAGE_KUBERNETES_AGENT_TOKEN" \
    --dry-run=client -o yaml | kubectl --kubeconfig "$kubeconfig" apply -f -
  after="$(kubectl --kubeconfig "$kubeconfig" get secret vantage-api-token -n vantage -o jsonpath='{.data.token}')"

  if [[ "$before" != "$after" && "$(kubectl --kubeconfig "$kubeconfig" get statefulset vka -n vantage --ignore-not-found)" != "" ]]; then
    kubectl --kubeconfig "$kubeconfig" rollout restart statefulset/vka -n vantage
  fi
}

ensure_eks_gp3_storage_class() {
  local kubeconfig="$1"

  kubectl --kubeconfig "$kubeconfig" annotate storageclass gp2 \
    storageclass.kubernetes.io/is-default-class=false \
    --overwrite >/dev/null 2>&1 || true

  kubectl --kubeconfig "$kubeconfig" apply -f - <<'YAML'
apiVersion: storage.k8s.io/v1
kind: StorageClass
metadata:
  name: gp3
  annotations:
    storageclass.kubernetes.io/is-default-class: "true"
provisioner: ebs.csi.aws.com
parameters:
  type: gp3
  fsType: ext4
reclaimPolicy: Delete
volumeBindingMode: WaitForFirstConsumer
allowVolumeExpansion: true
YAML
}

install_agent() {
  local provider="$1"
  local kubeconfig="$2"
  local cluster_id="$3"
  shift 3

  ensure_secret "$kubeconfig"

  helm upgrade --install vka vantage/vantage-kubernetes-agent \
    --kubeconfig "$kubeconfig" \
    --namespace vantage \
    --version "$chart_version" \
    --set agent.clusterID="$cluster_id" \
    --set agent.secret.name=vantage-api-token \
    --set agent.secret.key=token \
    --set resources.requests.cpu=100m \
    --set resources.requests.memory=100Mi \
    --set resources.limits.cpu=500m \
    --set resources.limits.memory=512Mi \
    "$@" \
    --wait \
    --timeout 4m

  if ! kubectl --kubeconfig "$kubeconfig" rollout status statefulset/vka -n vantage --timeout=4m; then
    kubectl --kubeconfig "$kubeconfig" get pods -n vantage -o wide
    kubectl --kubeconfig "$kubeconfig" get events -n vantage --sort-by=.lastTimestamp
    exit 1
  fi

  echo "Vantage Kubernetes Agent is ready on ${provider}: ${cluster_id}"
}

if [[ "$install_eks" == "true" ]]; then
  : "${ALIEN_TEST_EKS_KUBECONFIG:?ALIEN_TEST_EKS_KUBECONFIG is required}"
  : "${ALIEN_TEST_EKS_CLUSTER_NAME:?ALIEN_TEST_EKS_CLUSTER_NAME is required}"
  ensure_eks_gp3_storage_class "$ALIEN_TEST_EKS_KUBECONFIG"
  install_agent \
    "EKS" \
    "$ALIEN_TEST_EKS_KUBECONFIG" \
    "alien-e2e-aws-eks-${ALIEN_TEST_EKS_CLUSTER_NAME#alien-e2e-}" \
    --set persist.storageClassName=gp3
fi

if [[ "$install_aks" == "true" ]]; then
  : "${ALIEN_TEST_AKS_KUBECONFIG:?ALIEN_TEST_AKS_KUBECONFIG is required}"
  : "${ALIEN_TEST_AKS_CLUSTER_NAME:?ALIEN_TEST_AKS_CLUSTER_NAME is required}"
  install_agent \
    "AKS" \
    "$ALIEN_TEST_AKS_KUBECONFIG" \
    "alien-e2e-azure-aks-${ALIEN_TEST_AKS_CLUSTER_NAME#alien-e2e-}" \
    --set agent.disableKubeTLSverify=true \
    --set agent.nodeAddressTypes=InternalIP
fi
