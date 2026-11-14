#!/usr/bin/env bash
# Copy the released verifier image from ghcr.io to the registries protected
# installs pull it from, keeping its digest, and wait until ECR has it in every
# region.
#
#   scripts/publish-verifier-image.sh <source digest> <version>
#
#   VERIFIER_IMAGE_AWS_ACCOUNT      12-digit account whose ECR holds the image
#   VERIFIER_IMAGE_AWS_REPOSITORY   ECR repository name in that account (us-east-1)
#   VERIFIER_IMAGE_GCP_REPOSITORY   <host>/<project>/<repository>/<image> in Artifact Registry
#   GCP_ACCESS_TOKEN                token allowed to push to that repository
#   VERIFIER_SOURCE                 optional, defaults to ghcr.io/alienplatform/alien-verifier
#
# Signed setups name the image as <repository>@<digest>, and Lambda pulls only
# from ECR in its own region, so the copy must keep the manifest bytes: crane
# copies them unchanged, where `docker buildx imagetools create` would wrap a
# single manifest in a new index with a new digest. Needs crane, jq and the AWS
# CLI with credentials that may push to the ECR repository.
set -euo pipefail

digest="${1:?usage: publish-verifier-image.sh <source digest> <version>}"
version="${2:?usage: publish-verifier-image.sh <source digest> <version>}"
source_image="${VERIFIER_SOURCE:-ghcr.io/alienplatform/alien-verifier}"

errors=()
[[ "$digest" =~ ^sha256:[0-9a-f]{64}$ ]] || errors+=("source digest '$digest' is not sha256:<64 hex>")
[[ "${VERIFIER_IMAGE_AWS_ACCOUNT:-}" =~ ^[0-9]{12}$ ]] ||
  errors+=("VERIFIER_IMAGE_AWS_ACCOUNT is not a 12-digit AWS account")
[[ "${VERIFIER_IMAGE_AWS_REPOSITORY:-}" =~ ^[a-z0-9._/-]+$ ]] ||
  errors+=("VERIFIER_IMAGE_AWS_REPOSITORY is not an ECR repository name")
[[ "${VERIFIER_IMAGE_GCP_REPOSITORY:-}" =~ ^[a-z0-9-]+-docker\.pkg\.dev/[a-z0-9._/-]+$ ]] ||
  errors+=("VERIFIER_IMAGE_GCP_REPOSITORY is not <location>-docker.pkg.dev/<project>/<repository>/<image>")
[[ -n "${GCP_ACCESS_TOKEN:-}" ]] || errors+=("GCP_ACCESS_TOKEN is not set")
if ((${#errors[@]})); then
  printf '::error::%s\n' "${errors[@]}"
  exit 1
fi

ecr_host="${VERIFIER_IMAGE_AWS_ACCOUNT}.dkr.ecr.us-east-1.amazonaws.com"
aws ecr get-login-password --region us-east-1 |
  crane auth login "$ecr_host" --username AWS --password-stdin
printf '%s' "$GCP_ACCESS_TOKEN" |
  crane auth login "${VERIFIER_IMAGE_GCP_REPOSITORY%%/*}" --username oauth2accesstoken --password-stdin

# Tags are immutable in both registries, so a rerun finds the tag already there
# and only checks that it names the same image.
for target in "${ecr_host}/${VERIFIER_IMAGE_AWS_REPOSITORY}" "$VERIFIER_IMAGE_GCP_REPOSITORY"; do
  tagged="${target}:v${version}"
  if ! existing=$(crane digest "$tagged" 2>/dev/null); then
    crane copy "${source_image}@${digest}" "$tagged"
    existing=$(crane digest "$tagged")
  fi
  if [[ "$existing" != "$digest" ]]; then
    echo "::error::${tagged} is ${existing}, not the released verifier ${digest}"
    exit 1
  fi
  echo "${target}@${digest}"
done

# A protected install in a region the image hasn't reached yet can't create its
# verifier, so the release isn't done until replication is.
for _ in $(seq 1 60); do
  statuses=$(aws ecr describe-image-replication-status --region us-east-1 \
    --repository-name "$VERIFIER_IMAGE_AWS_REPOSITORY" --image-id "imageDigest=${digest}" \
    --query 'replicationStatuses[].[region,status]' --output json)
  if jq -e 'any(.[]; .[1] == "FAILED")' <<<"$statuses" >/dev/null; then
    echo "::error::ECR replication of ${digest} failed: $(jq -c 'map(select(.[1] == "FAILED"))' <<<"$statuses")"
    exit 1
  fi
  if jq -e 'length > 0 and all(.[]; .[1] == "COMPLETE")' <<<"$statuses" >/dev/null; then
    echo "Replicated to $(jq -r 'map(.[0]) | join(", ")' <<<"$statuses")"
    exit 0
  fi
  sleep 10
done
echo "::error::ECR replication of ${digest} didn't complete in 10 minutes: $(jq -c . <<<"$statuses")"
exit 1
