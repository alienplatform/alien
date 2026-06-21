#!/usr/bin/env bash
# Configure provider-native ingress for shared Kubernetes E2E clusters.
#
# Usage:
#   ./scripts/configure-e2e-provider-ingress.sh
#
# Terraform creates the clusters and emits kubeconfigs. Kubernetes API setup is
# intentionally done after `terraform apply`: provider-native ingress classes
# depend on cluster endpoints that are only usable after the apply has finished.
#
# Optional provider switches:
#   ALIEN_E2E_CONFIGURE_EKS=false
#   ALIEN_E2E_CONFIGURE_GKE=false
#   ALIEN_E2E_CONFIGURE_AKS=false
#
# Optional target selection:
#   ALIEN_E2E_GCP_TARGET=gcp-target-3
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

TF_JSON=$(cd infra/test && terraform output -json)
tmp_dir=$(mktemp -d)
trap 'rm -rf "$tmp_dir"' EXIT

jq_value() {
  local key="$1"
  printf '%s' "$TF_JSON" | jq -er --arg key "$key" '.[$key].value'
}

target_option() {
  local output="$1"
  local alias="$2"
  local key="$3"
  printf '%s' "$TF_JSON" | jq -er \
    --arg output "$output" \
    --arg alias "$alias" \
    --arg key "$key" \
    '.[$output].value[$alias][$key]'
}

write_kubeconfig() {
  local key="$1"
  local path="$2"
  jq_value "$key" > "$path"
  chmod 600 "$path"
}

should_configure_provider() {
  local value="${1:-true}"
  [[ "$value" == "true" || "$value" == "1" || "$value" == "yes" ]]
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

ensure_eks_gp3_default_storage_class() {
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

  echo "StorageClass gp3 is default for EKS"
}

ensure_default_storage_class() {
  local name="$1"
  local kubeconfig="$2"
  local preferred_class="${3:-}"
  local current_default
  local selected_class
  local class_count

  wait_for_kube_authorization "$name" "$kubeconfig"

  current_default=$(kubectl --kubeconfig "$kubeconfig" get storageclass -o json \
    | jq -r '.items[] | select(.metadata.annotations["storageclass.kubernetes.io/is-default-class"] == "true") | .metadata.name' \
    | head -n 1)
  if [[ -n "$current_default" ]]; then
    echo "StorageClass ${current_default} is already default for ${name}"
    return 0
  fi

  if [[ -n "$preferred_class" ]] && kubectl --kubeconfig "$kubeconfig" get storageclass "$preferred_class" >/dev/null 2>&1; then
    selected_class="$preferred_class"
  else
    class_count=$(kubectl --kubeconfig "$kubeconfig" get storageclass -o jsonpath='{range .items[*]}{.metadata.name}{"\n"}{end}' | wc -l | tr -d ' ')
    if [[ "$class_count" != "1" ]]; then
      echo "No default StorageClass for ${name}, and no unambiguous class to select" >&2
      kubectl --kubeconfig "$kubeconfig" get storageclass >&2
      return 1
    fi
    selected_class=$(kubectl --kubeconfig "$kubeconfig" get storageclass -o jsonpath='{.items[0].metadata.name}')
  fi

  kubectl --kubeconfig "$kubeconfig" annotate storageclass "$selected_class" \
    storageclass.kubernetes.io/is-default-class=true \
    --overwrite
  echo "StorageClass ${selected_class} is now default for ${name}"
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

if should_configure_provider "${ALIEN_E2E_CONFIGURE_EKS:-true}"; then
  eks_kubeconfig="$tmp_dir/eks.yaml"
  write_kubeconfig e2e_eks_kubeconfig "$eks_kubeconfig"
  ensure_eks_gp3_default_storage_class "$eks_kubeconfig"
  configure_eks_auto_mode_ingress "$eks_kubeconfig"
else
  echo "Skipping EKS provider-native Kubernetes configuration"
fi

if should_configure_provider "${ALIEN_E2E_CONFIGURE_GKE:-true}"; then
  gke_alias="${ALIEN_E2E_GCP_TARGET:-gcp-target-3}"
  gke_kubeconfig="$tmp_dir/gke.yaml"
  gke_key_file="$tmp_dir/gke-key.json"
  target_option gcp_target_options "$gke_alias" GOOGLE_TARGET_SERVICE_ACCOUNT_KEY > "$gke_key_file"
  ./scripts/write-gke-kubeconfig.sh \
    --key-file "$gke_key_file" \
    --project "$(target_option gcp_target_options "$gke_alias" GOOGLE_TARGET_PROJECT_ID)" \
    --cluster "$(target_option gcp_target_options "$gke_alias" ALIEN_TEST_GKE_CLUSTER_NAME)" \
    --location "$(target_option gcp_target_options "$gke_alias" ALIEN_TEST_GKE_CLUSTER_LOCATION)" \
    --kubeconfig "$gke_kubeconfig"
  ensure_default_storage_class "GKE" "$gke_kubeconfig"
else
  echo "Skipping GKE provider-native Kubernetes configuration"
fi

if should_configure_provider "${ALIEN_E2E_CONFIGURE_AKS:-true}"; then
  aks_kubeconfig="$tmp_dir/aks.yaml"
  write_kubeconfig e2e_aks_kubeconfig "$aks_kubeconfig"
  wait_for_kube_authorization "AKS" "$aks_kubeconfig"
  ensure_default_storage_class "AKS" "$aks_kubeconfig"
  wait_for_ingress_class "AKS" "$aks_kubeconfig" "webapprouting.kubernetes.azure.com"
else
  echo "Skipping AKS provider-native Kubernetes configuration"
fi

echo "Provider-native Kubernetes ingress and default storage classes are configured"
