#!/usr/bin/env bash
# Configure provider-native ingress for shared Kubernetes E2E clusters.
#
# Terraform creates the clusters and emits kubeconfigs. Kubernetes API setup is
# intentionally done after `terraform apply`: provider-native ingress classes
# depend on cluster endpoints that are only usable after the apply has finished.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

TF_JSON=$(cd infra/test && terraform output -json)
tmp_dir=$(mktemp -d)
trap 'rm -rf "$tmp_dir"' EXIT

jq_value() {
  local key="$1"
  printf '%s' "$TF_JSON" | jq -er --arg key "$key" '.[$key].value'
}

write_kubeconfig() {
  local key="$1"
  local path="$2"
  jq_value "$key" > "$path"
  chmod 600 "$path"
}

wait_for_kube_authorization() {
  local name="$1"
  local kubeconfig="$2"
  local attempt
  local max_attempts=40
  local delay_seconds=15

  for attempt in $(seq 1 "$max_attempts"); do
    if kubectl --kubeconfig "$kubeconfig" auth can-i get ingressclasses.networking.k8s.io >/dev/null 2>&1; then
      echo "Kubernetes authorization is ready for ${name}"
      return 0
    fi

    echo "Waiting for Kubernetes authorization for ${name} (${attempt}/${max_attempts})"
    sleep "$delay_seconds"
  done

  echo "Timed out waiting for Kubernetes authorization for ${name}" >&2
  kubectl --kubeconfig "$kubeconfig" auth can-i get ingressclasses.networking.k8s.io || true
  return 1
}

wait_for_ingress_class() {
  local name="$1"
  local kubeconfig="$2"
  local class="$3"
  local attempt
  local max_attempts=40
  local delay_seconds=15

  for attempt in $(seq 1 "$max_attempts"); do
    if kubectl --kubeconfig "$kubeconfig" get ingressclass "$class" >/dev/null 2>&1; then
      echo "IngressClass ${class} is ready for ${name}"
      return 0
    fi

    echo "Waiting for IngressClass ${class} for ${name} (${attempt}/${max_attempts})"
    sleep "$delay_seconds"
  done

  echo "Timed out waiting for IngressClass ${class} for ${name}" >&2
  kubectl --kubeconfig "$kubeconfig" get ingressclass || true
  return 1
}

configure_eks_auto_mode_ingress() {
  local kubeconfig="$1"

  wait_for_kube_authorization "EKS" "$kubeconfig"

  kubectl --kubeconfig "$kubeconfig" apply -f - <<'YAML'
apiVersion: eks.amazonaws.com/v1
kind: IngressClassParams
metadata:
  name: alb
spec:
  scheme: internet-facing
---
apiVersion: networking.k8s.io/v1
kind: IngressClass
metadata:
  name: alb
spec:
  controller: eks.amazonaws.com/alb
  parameters:
    apiGroup: eks.amazonaws.com
    kind: IngressClassParams
    name: alb
YAML

  wait_for_ingress_class "EKS" "$kubeconfig" "alb"
}

eks_kubeconfig="$tmp_dir/eks.yaml"
write_kubeconfig e2e_eks_kubeconfig "$eks_kubeconfig"
configure_eks_auto_mode_ingress "$eks_kubeconfig"

aks_kubeconfig="$tmp_dir/aks.yaml"
write_kubeconfig e2e_aks_kubeconfig "$aks_kubeconfig"
wait_for_kube_authorization "AKS" "$aks_kubeconfig"
wait_for_ingress_class "AKS" "$aks_kubeconfig" "webapprouting.kubernetes.azure.com"

echo "Provider-native Kubernetes ingress is configured"
