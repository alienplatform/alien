#!/usr/bin/env bash
# Install the shared ingress-nginx controller into E2E Kubernetes clusters.
#
# Terraform creates the clusters and emits kubeconfigs. Helm installation is
# intentionally done after `terraform apply`: Terraform providers cannot depend
# on cluster endpoints that are unknown until the same apply finishes.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

TF_JSON=$(cd infra/test && terraform output -json)
tmp_dir=$(mktemp -d)
trap 'rm -rf "$tmp_dir"' EXIT

jq_value() {
  local key="$1"
  printf '%s' "$TF_JSON" | jq -er --arg key "$key" '.[$key].value'
}

jq_csv() {
  local key="$1"
  jq_value "$key" | jq -er 'join(",")'
}

write_kubeconfig() {
  local key="$1"
  local path="$2"
  jq_value "$key" > "$path"
  chmod 600 "$path"
}

write_common_values() {
  local path="$1"
  jq -n --arg ingress_class "$(jq_value e2e_k8s_ingress_class)" '{
    controller: {
      ingressClass: $ingress_class,
      ingressClassResource: {
        enabled: true,
        name: $ingress_class
      },
      resources: {
        requests: {
          cpu: "100m",
          memory: "128Mi"
        },
        limits: {
          cpu: "500m",
          memory: "512Mi"
        }
      },
      admissionWebhooks: {
        enabled: false
      }
    }
  }' > "$path"
}

install_ingress() {
  local name="$1"
  local kubeconfig="$2"
  local values="$3"
  local version

  version=$(jq_value e2e_ingress_nginx_chart_version)
  echo "Installing ingress-nginx for ${name}"
  helm upgrade --install ingress-nginx ingress-nginx \
    --repo https://kubernetes.github.io/ingress-nginx \
    --version "$version" \
    --namespace ingress-nginx \
    --create-namespace \
    --wait \
    --timeout 10m \
    --kubeconfig "$kubeconfig" \
    -f "$values"
}

common_values="$tmp_dir/common-values.yaml"
write_common_values "$common_values"

eks_kubeconfig="$tmp_dir/eks.yaml"
write_kubeconfig e2e_eks_kubeconfig "$eks_kubeconfig"
jq -s \
  --arg eip_allocations "$(jq_csv e2e_eks_ingress_eip_allocation_ids)" \
  --arg subnets "$(jq_csv e2e_aws_public_subnet_ids)" \
  '.[0] * {
    controller: {
      service: {
        type: "LoadBalancer",
        annotations: {
          "service.beta.kubernetes.io/aws-load-balancer-eip-allocations": $eip_allocations,
          "service.beta.kubernetes.io/aws-load-balancer-nlb-target-type": "ip",
          "service.beta.kubernetes.io/aws-load-balancer-scheme": "internet-facing",
          "service.beta.kubernetes.io/aws-load-balancer-subnets": $subnets
        }
      }
    }
  }' "$common_values" > "$tmp_dir/eks-values.yaml"
install_ingress "EKS" "$eks_kubeconfig" "$tmp_dir/eks-values.yaml"

for target in 1 2 3; do
  gke_kubeconfig="$tmp_dir/gke-target-${target}.yaml"
  write_kubeconfig "e2e_gke_target_${target}_kubeconfig" "$gke_kubeconfig"
  jq -s \
    --arg ip "$(jq_value "e2e_gke_target_${target}_ingress_ip_address")" \
    '.[0] * { controller: { service: { loadBalancerIP: $ip } } }' \
    "$common_values" > "$tmp_dir/gke-target-${target}-values.yaml"
  install_ingress "GKE target ${target}" "$gke_kubeconfig" "$tmp_dir/gke-target-${target}-values.yaml"
done

aks_kubeconfig="$tmp_dir/aks.yaml"
write_kubeconfig e2e_aks_kubeconfig "$aks_kubeconfig"
jq -s \
  --arg resource_group "$(jq_value e2e_aks_cluster_resource_group)" \
  --arg public_ip_name "$(jq_value e2e_aks_ingress_public_ip_name)" \
  '.[0] * {
    controller: {
      service: {
        annotations: {
          "service.beta.kubernetes.io/azure-load-balancer-resource-group": $resource_group,
          "service.beta.kubernetes.io/azure-pip-name": $public_ip_name
        }
      }
    }
  }' "$common_values" > "$tmp_dir/aks-values.yaml"
install_ingress "AKS" "$aks_kubeconfig" "$tmp_dir/aks-values.yaml"
