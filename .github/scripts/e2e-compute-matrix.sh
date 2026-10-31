#!/usr/bin/env bash
# Keep matrix computation outside the workflow expression size limit.
set -euo pipefail

# Determine whether tests should run at all based on event type
SHOULD_RUN=false
case "${MATRIX_EVENT_NAME}" in
  workflow_dispatch|merge_group)
    SHOULD_RUN=true
    ;;
  pull_request)
    IS_FORK="${MATRIX_IS_FORK}"
    HAS_LABEL="${MATRIX_HAS_LABEL}"
    if [ "$IS_FORK" = "false" ] && [ "$HAS_LABEL" = "true" ]; then
      SHOULD_RUN=true
    fi
    ;;
esac

if [ "$SHOULD_RUN" = "false" ]; then
  echo 'push_matrix={"include":[]}' >> $GITHUB_OUTPUT
  echo 'local_matrix={"include":[]}' >> $GITHUB_OUTPUT
  echo 'terraform_matrix={"include":[]}' >> $GITHUB_OUTPUT
  echo 'cloudformation_matrix={"include":[]}' >> $GITHUB_OUTPUT
  echo "has_push=false" >> $GITHUB_OUTPUT
  echo "has_local=false" >> $GITHUB_OUTPUT
  echo "has_terraform=false" >> $GITHUB_OUTPUT
  echo "has_cloudformation=false" >> $GITHUB_OUTPUT
  echo "needs_x86_64=false" >> $GITHUB_OUTPUT
  echo "needs_aarch64=false" >> $GITHUB_OUTPUT
  echo "needs_bindings_x86_64=false" >> $GITHUB_OUTPUT
  echo "needs_bindings_aarch64=false" >> $GITHUB_OUTPUT
  echo "build_images=true" >> $GITHUB_OUTPUT
  echo "build_base_image=true" >> $GITHUB_OUTPUT
  echo "build_operator_image=true" >> $GITHUB_OUTPUT
  echo "skip_cleanup=true" >> $GITHUB_OUTPUT
  echo "test_filter=" >> $GITHUB_OUTPUT
  echo "network_mode=none" >> $GITHUB_OUTPUT
  exit 0
fi

# For workflow_dispatch, use explicit checkboxes for the product
# flows.
if [ "${MATRIX_EVENT_NAME}" = "workflow_dispatch" ]; then
  PUSH_AWS_CLI="${MATRIX_RUN_PUSH_AWS_CLI}"
  PUSH_GCP_CLI="${MATRIX_RUN_PUSH_GCP_CLI}"
  PUSH_AZURE_CLI="${MATRIX_RUN_PUSH_AZURE_CLI}"
  PUSH_AWS_CLOUDFORMATION="${MATRIX_RUN_PUSH_AWS_CLOUDFORMATION}"
  PUSH_AWS_TERRAFORM="${MATRIX_RUN_PUSH_AWS_TERRAFORM}"
  PUSH_GCP_TERRAFORM="${MATRIX_RUN_PUSH_GCP_TERRAFORM}"
  PUSH_AZURE_TERRAFORM="${MATRIX_RUN_PUSH_AZURE_TERRAFORM}"
  KUBERNETES_AWS_TERRAFORM_HELM="${MATRIX_RUN_KUBERNETES_AWS_TERRAFORM_HELM}"
  KUBERNETES_GCP_TERRAFORM_HELM="${MATRIX_RUN_KUBERNETES_GCP_TERRAFORM_HELM}"
  KUBERNETES_AZURE_TERRAFORM_HELM="${MATRIX_RUN_KUBERNETES_AZURE_TERRAFORM_HELM}"
  LOCAL_LINUX_AGENT="${MATRIX_RUN_LOCAL_LINUX_AGENT}"
  LOCAL_MACOS_AGENT="${MATRIX_RUN_LOCAL_MACOS_AGENT}"
  LOCAL_WINDOWS_AGENT="${MATRIX_RUN_LOCAL_WINDOWS_AGENT}"
  APP="${MATRIX_APP}"
  NETWORK_MODE="${MATRIX_NETWORK_MODE}"
  BUILD_IMAGES="${MATRIX_BUILD_IMAGES}"
  BASE_IMAGE_OVERRIDE="${MATRIX_BASE_IMAGE_OVERRIDE}"
  OPERATOR_IMAGE_OVERRIDE="${MATRIX_OPERATOR_IMAGE_OVERRIDE}"
  SKIP_CLEANUP="${MATRIX_SKIP_CLEANUP}"
  TEST_FILTER="${MATRIX_TEST_FILTER}"
else
  PUSH_AWS_CLI=true
  PUSH_GCP_CLI=false
  PUSH_AZURE_CLI=false
  PUSH_AWS_CLOUDFORMATION=false
  PUSH_AWS_TERRAFORM=false
  PUSH_GCP_TERRAFORM=false
  PUSH_AZURE_TERRAFORM=false
  KUBERNETES_AWS_TERRAFORM_HELM=false
  KUBERNETES_GCP_TERRAFORM_HELM=false
  KUBERNETES_AZURE_TERRAFORM_HELM=false
  LOCAL_LINUX_AGENT=true
  LOCAL_MACOS_AGENT=false
  LOCAL_WINDOWS_AGENT=false
  APP=comprehensive-rust; NETWORK_MODE=none; BUILD_IMAGES=true; BASE_IMAGE_OVERRIDE=""; OPERATOR_IMAGE_OVERRIDE=""; SKIP_CLEANUP=false; TEST_FILTER=""
fi

BUILD_BASE_IMAGE=$BUILD_IMAGES
BUILD_AGENT_IMAGE=$BUILD_IMAGES
if [ "$BUILD_IMAGES" != "true" ]; then
  BUILD_BASE_IMAGE=false
  BUILD_AGENT_IMAGE=false
fi
if [ -n "$BASE_IMAGE_OVERRIDE" ]; then
  BUILD_BASE_IMAGE=false
fi
if [ -n "$OPERATOR_IMAGE_OVERRIDE" ]; then
  BUILD_AGENT_IMAGE=false
fi
if [ "$BUILD_BASE_IMAGE" != "true" ] && [ "$BUILD_AGENT_IMAGE" != "true" ]; then
  BUILD_IMAGES=false
fi

# Build push matrix
PUSH_ENTRIES='[]'
if [ "$PUSH_AWS_CLI" = "true" ]; then
  if [ "$APP" = "All" ] || [ "$APP" = "storage-notifications" ]; then
    PUSH_ENTRIES=$(echo "$PUSH_ENTRIES" | jq -c '. + [{"name":"push-aws-storage-notifications","test_filter":"push_aws_storage_notifications","needs_oidc":false,"resource_suffix":"awsntf","bindings_arch":"aarch64","timeout_minutes":60}]')
  fi
  if [ "$APP" = "All" ] || [ "$APP" = "automatic-setup" ]; then
    PUSH_ENTRIES=$(echo "$PUSH_ENTRIES" | jq -c '. + [{"name":"push-aws-automatic-setup","test_filter":"push_aws_automatic_setup","needs_oidc":false,"resource_suffix":"autoaws","timeout_minutes":60}]')
  fi
  if [ "$APP" = "All" ] || [ "$APP" = "comprehensive-rust" ]; then
    PUSH_ENTRIES=$(echo "$PUSH_ENTRIES" | jq -c '. + [{"name":"push-aws-comprehensive-rust","test_filter":"push_aws_comprehensive_rust","needs_oidc":false,"resource_suffix":"awscr","timeout_minutes":60}]')
  fi
  if [ "$APP" = "All" ] || [ "$APP" = "comprehensive-ts" ]; then
    PUSH_ENTRIES=$(echo "$PUSH_ENTRIES" | jq -c '. + [{"name":"push-aws-comprehensive-ts","test_filter":"push_aws_comprehensive_ts","needs_oidc":false,"resource_suffix":"awsct","bindings_arch":"aarch64","timeout_minutes":60}]')
  fi
fi
if [ "$PUSH_GCP_CLI" = "true" ]; then
  if [ "$APP" = "All" ] || [ "$APP" = "storage-notifications" ]; then
    PUSH_ENTRIES=$(echo "$PUSH_ENTRIES" | jq -c '. + [{"name":"push-gcp-storage-notifications","test_filter":"push_gcp_storage_notifications","needs_oidc":false,"resource_suffix":"gcpntf","bindings_arch":"x86_64","timeout_minutes":60}]')
  fi
  if [ "$APP" = "All" ] || [ "$APP" = "automatic-setup" ]; then
    PUSH_ENTRIES=$(echo "$PUSH_ENTRIES" | jq -c '. + [{"name":"push-gcp-automatic-setup","test_filter":"push_gcp_automatic_setup","needs_oidc":false,"resource_suffix":"autogcp","timeout_minutes":60}]')
  fi
  if [ "$APP" = "All" ] || [ "$APP" = "comprehensive-rust" ]; then
    PUSH_ENTRIES=$(echo "$PUSH_ENTRIES" | jq -c '. + [{"name":"push-gcp-comprehensive-rust","test_filter":"push_gcp_comprehensive_rust","needs_oidc":false,"resource_suffix":"gcpcr","timeout_minutes":60}]')
  fi
  if [ "$APP" = "All" ] || [ "$APP" = "comprehensive-ts" ]; then
    PUSH_ENTRIES=$(echo "$PUSH_ENTRIES" | jq -c '. + [{"name":"push-gcp-comprehensive-ts","test_filter":"push_gcp_comprehensive_ts","needs_oidc":false,"resource_suffix":"gcpct","bindings_arch":"x86_64","timeout_minutes":60}]')
  fi
fi
if [ "$PUSH_AZURE_CLI" = "true" ]; then
  if [ "$APP" = "All" ] || [ "$APP" = "storage-notifications" ]; then
    PUSH_ENTRIES=$(echo "$PUSH_ENTRIES" | jq -c '. + [{"name":"push-azure-storage-notifications","test_filter":"push_azure_storage_notifications","needs_oidc":true,"resource_suffix":"azurentf","bindings_arch":"x86_64","timeout_minutes":60}]')
  fi
  if [ "$APP" = "All" ] || [ "$APP" = "automatic-setup" ]; then
    PUSH_ENTRIES=$(echo "$PUSH_ENTRIES" | jq -c '. + [{"name":"push-azure-automatic-setup","test_filter":"push_azure_automatic_setup","needs_oidc":true,"resource_suffix":"autoaz","timeout_minutes":60}]')
  fi
  if [ "$APP" = "All" ] || [ "$APP" = "comprehensive-rust" ]; then
    PUSH_ENTRIES=$(echo "$PUSH_ENTRIES" | jq -c '. + [{"name":"push-azure-comprehensive-rust","test_filter":"push_azure_comprehensive_rust","needs_oidc":true,"resource_suffix":"azcr","timeout_minutes":75}]')
  fi
  if [ "$APP" = "All" ] || [ "$APP" = "comprehensive-ts" ]; then
    PUSH_ENTRIES=$(echo "$PUSH_ENTRIES" | jq -c '. + [{"name":"push-azure-comprehensive-ts","test_filter":"push_azure_comprehensive_ts","needs_oidc":true,"resource_suffix":"azct","bindings_arch":"x86_64","timeout_minutes":75}]')
  fi
fi

HAS_PUSH=$([ "$(echo "$PUSH_ENTRIES" | jq 'length')" -gt 0 ] && echo true || echo false)

# Build local matrix (native agent on each OS)
LOCAL_ENTRIES='[]'
if [ "$LOCAL_LINUX_AGENT" = "true" ]; then
  if [ "$APP" = "All" ] || [ "$APP" = "comprehensive-rust" ]; then
    LOCAL_ENTRIES=$(echo "$LOCAL_ENTRIES" | jq -c '. + [{"name":"local-linux-comprehensive-rust","os":"depot-ubuntu-24.04-arm-8","test_filter":"pull_local_comprehensive_rust"}]')
  fi
  if [ "$APP" = "All" ] || [ "$APP" = "comprehensive-ts" ]; then
    LOCAL_ENTRIES=$(echo "$LOCAL_ENTRIES" | jq -c '. + [{"name":"local-linux-comprehensive-ts","os":"depot-ubuntu-24.04-arm-8","test_filter":"pull_local_comprehensive_ts"}]')
  fi
  if [ "$APP" = "All" ] || [ "$APP" = "command-routing-ts" ]; then
    LOCAL_ENTRIES=$(echo "$LOCAL_ENTRIES" | jq -c '. + [{"name":"local-linux-command-routing-ts","os":"depot-ubuntu-24.04-arm-8","test_filter":"pull_local_command_routing_ts"}]')
  fi
  if [ "$APP" = "All" ] || [ "$APP" = "container-rust" ]; then
    LOCAL_ENTRIES=$(echo "$LOCAL_ENTRIES" | jq -c '. + [{"name":"local-linux-container-rust","os":"depot-ubuntu-24.04-arm-8","test_filter":"pull_local_container_rust"}]')
  fi
  if [ "$APP" = "All" ] || [ "$APP" = "runtime-less-mixed" ]; then
    LOCAL_ENTRIES=$(echo "$LOCAL_ENTRIES" | jq -c '. + [{"name":"local-linux-runtime-less-mixed","os":"depot-ubuntu-24.04-arm-8","test_filter":"pull_local_runtime_less_mixed"}]')
  fi
fi
if [ "$LOCAL_MACOS_AGENT" = "true" ]; then
  if [ "$APP" = "All" ] || [ "$APP" = "comprehensive-rust" ]; then
    LOCAL_ENTRIES=$(echo "$LOCAL_ENTRIES" | jq -c '. + [{"name":"local-macos-comprehensive-rust","os":"depot-macos-latest","test_filter":"pull_local_comprehensive_rust"}]')
  fi
  if [ "$APP" = "All" ] || [ "$APP" = "comprehensive-ts" ]; then
    LOCAL_ENTRIES=$(echo "$LOCAL_ENTRIES" | jq -c '. + [{"name":"local-macos-comprehensive-ts","os":"depot-macos-latest","test_filter":"pull_local_comprehensive_ts"}]')
  fi
fi
if [ "$LOCAL_WINDOWS_AGENT" = "true" ]; then
  if [ "$APP" = "All" ] || [ "$APP" = "comprehensive-rust" ]; then
    LOCAL_ENTRIES=$(echo "$LOCAL_ENTRIES" | jq -c '. + [{"name":"local-windows-comprehensive-rust","os":"depot-windows-2025-8","test_filter":"pull_local_comprehensive_rust"}]')
  fi
  if [ "$APP" = "All" ] || [ "$APP" = "comprehensive-ts" ]; then
    LOCAL_ENTRIES=$(echo "$LOCAL_ENTRIES" | jq -c '. + [{"name":"local-windows-comprehensive-ts","os":"depot-windows-2025-8","test_filter":"pull_local_comprehensive_ts"}]')
  fi
fi

HAS_LOCAL=$([ "$(echo "$LOCAL_ENTRIES" | jq 'length')" -gt 0 ] && echo true || echo false)

# Build Terraform distribution matrix. Kubernetes entries provision
# frozen resources with Terraform and install the agent with Helm.
TERRAFORM_ENTRIES='[]'
if [ "$APP" = "All" ] || [ "$APP" = "storage-notifications" ]; then
  if [ "$PUSH_AWS_TERRAFORM" = "true" ]; then
    TERRAFORM_ENTRIES=$(echo "$TERRAFORM_ENTRIES" | jq -c '. + [{"name":"terraform-aws-push-storage-notifications","test_filter":"terraform_aws_push_storage_notifications","needs_oidc":false,"resource_suffix":"tfawsntf","bindings_arch":"aarch64"}]')
  fi
  if [ "$PUSH_GCP_TERRAFORM" = "true" ]; then
    TERRAFORM_ENTRIES=$(echo "$TERRAFORM_ENTRIES" | jq -c '. + [{"name":"terraform-gcp-push-storage-notifications","test_filter":"terraform_gcp_push_storage_notifications","needs_oidc":false,"resource_suffix":"tfgcpntf","bindings_arch":"x86_64"}]')
  fi
  if [ "$PUSH_AZURE_TERRAFORM" = "true" ]; then
    TERRAFORM_ENTRIES=$(echo "$TERRAFORM_ENTRIES" | jq -c '. + [{"name":"terraform-azure-push-storage-notifications","test_filter":"terraform_azure_push_storage_notifications","needs_oidc":true,"resource_suffix":"tfazurentf","bindings_arch":"x86_64"}]')
  fi
fi
if [ "$APP" = "All" ] || [ "$APP" = "automatic-setup" ]; then
  if [ "$PUSH_AWS_TERRAFORM" = "true" ]; then
    TERRAFORM_ENTRIES=$(echo "$TERRAFORM_ENTRIES" | jq -c '. + [{"name":"terraform-aws-push-automatic-setup","test_filter":"terraform_aws_push_automatic_setup","needs_oidc":false,"resource_suffix":"tfawsauto"}]')
  fi
  if [ "$PUSH_GCP_TERRAFORM" = "true" ]; then
    TERRAFORM_ENTRIES=$(echo "$TERRAFORM_ENTRIES" | jq -c '. + [{"name":"terraform-gcp-push-automatic-setup","test_filter":"terraform_gcp_push_automatic_setup","needs_oidc":false,"resource_suffix":"tfgcpauto"}]')
  fi
  if [ "$PUSH_AZURE_TERRAFORM" = "true" ]; then
    TERRAFORM_ENTRIES=$(echo "$TERRAFORM_ENTRIES" | jq -c '. + [{"name":"terraform-azure-push-automatic-setup","test_filter":"terraform_azure_push_automatic_setup","needs_oidc":true,"resource_suffix":"tfazauto"}]')
  fi
fi
if [ "$APP" = "All" ] || [ "$APP" = "comprehensive-rust" ]; then
  if [ "$PUSH_AWS_TERRAFORM" = "true" ]; then
    TERRAFORM_ENTRIES=$(echo "$TERRAFORM_ENTRIES" | jq -c '. + [{"name":"terraform-aws-push-comprehensive-rust","test_filter":"terraform_aws_push_comprehensive_rust","needs_oidc":false,"resource_suffix":"tfawscr"}]')
  fi
  if [ "$PUSH_GCP_TERRAFORM" = "true" ]; then
    TERRAFORM_ENTRIES=$(echo "$TERRAFORM_ENTRIES" | jq -c '. + [{"name":"terraform-gcp-push-comprehensive-rust","test_filter":"terraform_gcp_push_comprehensive_rust","needs_oidc":false,"resource_suffix":"tfgcpcr"}]')
  fi
  if [ "$PUSH_AZURE_TERRAFORM" = "true" ]; then
    TERRAFORM_ENTRIES=$(echo "$TERRAFORM_ENTRIES" | jq -c '. + [{"name":"terraform-azure-push-comprehensive-rust","test_filter":"terraform_azure_push_comprehensive_rust","needs_oidc":true,"resource_suffix":"tfazcr"}]')
  fi
  if [ "$KUBERNETES_AWS_TERRAFORM_HELM" = "true" ]; then
    TERRAFORM_ENTRIES=$(echo "$TERRAFORM_ENTRIES" | jq -c '. + [{"name":"terraform-eks-helm-pull-comprehensive-rust","test_filter":"terraform_eks_helm_pull_comprehensive_rust","needs_oidc":false,"resource_suffix":"tfekscr"}]')
  fi
  if [ "$KUBERNETES_GCP_TERRAFORM_HELM" = "true" ]; then
    TERRAFORM_ENTRIES=$(echo "$TERRAFORM_ENTRIES" | jq -c '. + [{"name":"terraform-gke-helm-pull-comprehensive-rust","test_filter":"terraform_gke_helm_pull_comprehensive_rust","needs_oidc":false,"resource_suffix":"tfgkecr"}]')
  fi
  if [ "$KUBERNETES_AZURE_TERRAFORM_HELM" = "true" ]; then
    TERRAFORM_ENTRIES=$(echo "$TERRAFORM_ENTRIES" | jq -c '. + [{"name":"terraform-aks-helm-pull-comprehensive-rust","test_filter":"terraform_aks_helm_pull_comprehensive_rust","needs_oidc":true,"resource_suffix":"tfakscr"}]')
  fi
fi
# AWS alone proves this app's contract. Which resources get stripped is
# decided by cloud-agnostic deployment code, and the rendered Terraform
# for a gated resource is linted by `terraform validate` on all three
# providers in alien-terraform's generator tests. Only the applied
# declined state is provider-specific, and proving it on GCP and Azure
# too would triple this app's cloud cost.
if [ "$APP" = "All" ] || [ "$APP" = "enabled-demo" ]; then
  if [ "$PUSH_AWS_TERRAFORM" = "true" ]; then
    TERRAFORM_ENTRIES=$(echo "$TERRAFORM_ENTRIES" | jq -c '. + [{"name":"terraform-aws-push-enabled-demo","test_filter":"terraform_aws_push_enabled_demo","needs_oidc":false,"resource_suffix":"tfawsed","bindings_arch":"aarch64"}]')
  fi
fi
# BYO AI must run once on every provider because each cloud has a
# different Remote Bindings identity, token mint, and inference API.
if [ "$APP" = "All" ] || [ "$APP" = "byo-ai" ]; then
  if [ "$PUSH_AWS_TERRAFORM" = "true" ]; then
    TERRAFORM_ENTRIES=$(echo "$TERRAFORM_ENTRIES" | jq -c '. + [{"name":"terraform-aws-push-byo-ai","test_filter":"terraform_aws_push_byo_ai","needs_oidc":false,"resource_suffix":"tfawsai"}]')
  fi
  if [ "$PUSH_GCP_TERRAFORM" = "true" ]; then
    TERRAFORM_ENTRIES=$(echo "$TERRAFORM_ENTRIES" | jq -c '. + [{"name":"terraform-gcp-push-byo-ai","test_filter":"terraform_gcp_push_byo_ai","needs_oidc":false,"resource_suffix":"tfgcpai"}]')
  fi
  if [ "$PUSH_AZURE_TERRAFORM" = "true" ]; then
    TERRAFORM_ENTRIES=$(echo "$TERRAFORM_ENTRIES" | jq -c '. + [{"name":"terraform-azure-push-byo-ai","test_filter":"terraform_azure_push_byo_ai","needs_oidc":true,"resource_suffix":"tfazai"}]')
  fi
fi
if [ "$APP" = "All" ] || [ "$APP" = "full-stack-microservices" ]; then
  if [ "$KUBERNETES_AWS_TERRAFORM_HELM" = "true" ]; then
    TERRAFORM_ENTRIES=$(echo "$TERRAFORM_ENTRIES" | jq -c '. + [{"name":"terraform-eks-helm-pull-full-stack-microservices","test_filter":"terraform_eks_helm_pull_full_stack_microservices","needs_oidc":false,"resource_suffix":"tfeksfs","bindings_arch":"aarch64"}]')
  fi
  if [ "$KUBERNETES_GCP_TERRAFORM_HELM" = "true" ]; then
    TERRAFORM_ENTRIES=$(echo "$TERRAFORM_ENTRIES" | jq -c '. + [{"name":"terraform-gke-helm-pull-full-stack-microservices","test_filter":"terraform_gke_helm_pull_full_stack_microservices","needs_oidc":false,"resource_suffix":"tfgkefs","bindings_arch":"x86_64"}]')
  fi
  if [ "$KUBERNETES_AZURE_TERRAFORM_HELM" = "true" ]; then
    TERRAFORM_ENTRIES=$(echo "$TERRAFORM_ENTRIES" | jq -c '. + [{"name":"terraform-aks-helm-pull-full-stack-microservices","test_filter":"terraform_aks_helm_pull_full_stack_microservices","needs_oidc":true,"resource_suffix":"tfaksfs","bindings_arch":"x86_64"}]')
  fi
fi
# One managed Kubernetes target is enough to prove the runtime-less
# Daemon and target-scoped routing contract. The app is portable to
# GKE/AKS too, but duplicating this behavior test across all three
# providers would only multiply cloud cost and runtime.
if [ "$APP" = "All" ] || [ "$APP" = "command-routing-ts" ]; then
  if [ "$KUBERNETES_AWS_TERRAFORM_HELM" = "true" ]; then
    TERRAFORM_ENTRIES=$(echo "$TERRAFORM_ENTRIES" | jq -c '. + [{"name":"terraform-eks-helm-pull-command-routing-ts","test_filter":"terraform_eks_helm_pull_command_routing_ts","needs_oidc":false,"resource_suffix":"tfeksrt","bindings_arch":"aarch64"}]')
  fi
fi
# The mixed app proves both missing runtime-less SDK paths on each
# managed Kubernetes provider: TypeScript Container + Rust Daemon,
# direct KV, pull receivers, and a target-scoped command-name
# collision.
if [ "$APP" = "All" ] || [ "$APP" = "runtime-less-mixed" ]; then
  if [ "$KUBERNETES_AWS_TERRAFORM_HELM" = "true" ]; then
    TERRAFORM_ENTRIES=$(echo "$TERRAFORM_ENTRIES" | jq -c '. + [{"name":"terraform-eks-helm-pull-runtime-less-mixed","test_filter":"terraform_eks_helm_pull_runtime_less_mixed","needs_oidc":false,"resource_suffix":"tfeksrm","bindings_arch":"aarch64"}]')
  fi
  if [ "$KUBERNETES_GCP_TERRAFORM_HELM" = "true" ]; then
    TERRAFORM_ENTRIES=$(echo "$TERRAFORM_ENTRIES" | jq -c '. + [{"name":"terraform-gke-helm-pull-runtime-less-mixed","test_filter":"terraform_gke_helm_pull_runtime_less_mixed","needs_oidc":false,"resource_suffix":"tfgkerm","bindings_arch":"x86_64"}]')
  fi
  if [ "$KUBERNETES_AZURE_TERRAFORM_HELM" = "true" ]; then
    TERRAFORM_ENTRIES=$(echo "$TERRAFORM_ENTRIES" | jq -c '. + [{"name":"terraform-aks-helm-pull-runtime-less-mixed","test_filter":"terraform_aks_helm_pull_runtime_less_mixed","needs_oidc":true,"resource_suffix":"tfaksrm","bindings_arch":"x86_64"}]')
  fi
fi

# Build CloudFormation distribution matrix. CloudFormation is AWS-only
# and currently validates the push/import path.
CLOUDFORMATION_ENTRIES='[]'
if [ "$PUSH_AWS_CLOUDFORMATION" = "true" ]; then
  if [ "$APP" = "All" ] || [ "$APP" = "storage-notifications" ]; then
    CLOUDFORMATION_ENTRIES=$(echo "$CLOUDFORMATION_ENTRIES" | jq -c '. + [{"name":"cloudformation-aws-push-storage-notifications","test_filter":"cloudformation_aws_push_storage_notifications","needs_oidc":false,"resource_suffix":"cfawsntf","bindings_arch":"aarch64"}]')
  fi
  if [ "$APP" = "All" ] || [ "$APP" = "automatic-setup" ]; then
    CLOUDFORMATION_ENTRIES=$(echo "$CLOUDFORMATION_ENTRIES" | jq -c '. + [{"name":"cloudformation-aws-push-automatic-setup","test_filter":"cloudformation_aws_push_automatic_setup","needs_oidc":false,"resource_suffix":"cfawsauto"}]')
  fi
  if [ "$APP" = "All" ] || [ "$APP" = "comprehensive-rust" ]; then
    CLOUDFORMATION_ENTRIES=$(echo "$CLOUDFORMATION_ENTRIES" | jq -c '. + [{"name":"cloudformation-aws-push-comprehensive-rust","test_filter":"cloudformation_aws_push_comprehensive_rust","needs_oidc":false,"resource_suffix":"cfawscr"}]')
  fi
fi

HAS_TERRAFORM=$([ "$(echo "$TERRAFORM_ENTRIES" | jq 'length')" -gt 0 ] && echo true || echo false)
HAS_CLOUDFORMATION=$([ "$(echo "$CLOUDFORMATION_ENTRIES" | jq 'length')" -gt 0 ] && echo true || echo false)

# TypeScript resources compile the in-process bindings addon into the
# target executable. Build each Linux addon natively: the ARM E2E
# runner cannot cross-build the x86_64 napi cdylib (or vice versa).
NEEDS_BINDINGS_X86_64=false
NEEDS_BINDINGS_AARCH64=false
if jq -n -e --argjson push "$PUSH_ENTRIES" --argjson terraform "$TERRAFORM_ENTRIES" --argjson cloudformation "$CLOUDFORMATION_ENTRIES" \
  'any(($push + $terraform + $cloudformation)[]; .bindings_arch? == "aarch64")' >/dev/null; then
  NEEDS_BINDINGS_AARCH64=true
fi
if jq -n -e --argjson push "$PUSH_ENTRIES" --argjson terraform "$TERRAFORM_ENTRIES" --argjson cloudformation "$CLOUDFORMATION_ENTRIES" \
  'any(($push + $terraform + $cloudformation)[]; .bindings_arch? == "x86_64")' >/dev/null; then
  NEEDS_BINDINGS_X86_64=true
fi

# Architecture needs: AWS→aarch64, GCP/Azure→x86_64
ANY_TESTS=false
if [ "$HAS_PUSH" = "true" ] || [ "$HAS_TERRAFORM" = "true" ] || [ "$HAS_CLOUDFORMATION" = "true" ]; then ANY_TESTS=true; fi

NEEDS_X86_64=false
NEEDS_AARCH64=false
if [ "$ANY_TESTS" = "true" ]; then
  if [ "$PUSH_GCP_CLI" = "true" ] || [ "$PUSH_AZURE_CLI" = "true" ]; then NEEDS_X86_64=true; fi
  if [ "$PUSH_AWS_CLI" = "true" ]; then NEEDS_AARCH64=true; fi
fi
# Distribution tests run on ARM runners. K8s Terraform+Helm tests also
# consume the operator image, so keep both architectures available.
if [ "$HAS_TERRAFORM" = "true" ]; then NEEDS_X86_64=true; NEEDS_AARCH64=true; fi
if [ "$HAS_CLOUDFORMATION" = "true" ]; then NEEDS_AARCH64=true; fi

echo "push_matrix=$(echo "$PUSH_ENTRIES" | jq -c '{include: .}')" >> $GITHUB_OUTPUT
echo "local_matrix=$(echo "$LOCAL_ENTRIES" | jq -c '{include: .}')" >> $GITHUB_OUTPUT
echo "terraform_matrix=$(echo "$TERRAFORM_ENTRIES" | jq -c '{include: .}')" >> $GITHUB_OUTPUT
echo "cloudformation_matrix=$(echo "$CLOUDFORMATION_ENTRIES" | jq -c '{include: .}')" >> $GITHUB_OUTPUT
echo "has_push=$HAS_PUSH" >> $GITHUB_OUTPUT
echo "has_local=$HAS_LOCAL" >> $GITHUB_OUTPUT
echo "has_terraform=$HAS_TERRAFORM" >> $GITHUB_OUTPUT
echo "has_cloudformation=$HAS_CLOUDFORMATION" >> $GITHUB_OUTPUT
echo "needs_x86_64=$NEEDS_X86_64" >> $GITHUB_OUTPUT
echo "needs_aarch64=$NEEDS_AARCH64" >> $GITHUB_OUTPUT
echo "needs_bindings_x86_64=$NEEDS_BINDINGS_X86_64" >> $GITHUB_OUTPUT
echo "needs_bindings_aarch64=$NEEDS_BINDINGS_AARCH64" >> $GITHUB_OUTPUT
echo "build_images=$BUILD_IMAGES" >> $GITHUB_OUTPUT
echo "build_base_image=$BUILD_BASE_IMAGE" >> $GITHUB_OUTPUT
echo "build_operator_image=$BUILD_AGENT_IMAGE" >> $GITHUB_OUTPUT
echo "skip_cleanup=$SKIP_CLEANUP" >> $GITHUB_OUTPUT
echo "test_filter=$TEST_FILTER" >> $GITHUB_OUTPUT
echo "network_mode=$NETWORK_MODE" >> $GITHUB_OUTPUT
