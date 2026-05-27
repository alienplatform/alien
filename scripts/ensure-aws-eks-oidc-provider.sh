#!/usr/bin/env bash
set -euo pipefail

if [ -z "${ALIEN_TEST_EKS_CLUSTER_NAME:-}" ]; then
  echo "ALIEN_TEST_EKS_CLUSTER_NAME must be set" >&2
  exit 1
fi

issuer=$(aws eks describe-cluster \
  --name "$ALIEN_TEST_EKS_CLUSTER_NAME" \
  --query 'cluster.identity.oidc.issuer' \
  --output text)

if [ -z "$issuer" ] || [ "$issuer" = "None" ]; then
  echo "EKS cluster has no OIDC issuer: $ALIEN_TEST_EKS_CLUSTER_NAME" >&2
  exit 1
fi

issuer_host_path="${issuer#https://}"
issuer_host="${issuer_host_path%%/*}"

providers=$(aws iam list-open-id-connect-providers \
  --query 'OpenIDConnectProviderList[].Arn' \
  --output text) || providers=""

for provider_arn in $providers; do
  provider_url=$(aws iam get-open-id-connect-provider \
    --open-id-connect-provider-arn "$provider_arn" \
    --query 'Url' \
    --output text 2>/dev/null) || continue
  if [ "$provider_url" = "$issuer_host_path" ]; then
    echo "EKS OIDC provider already exists: $provider_arn"
    exit 0
  fi
done

workdir=$(mktemp -d)
trap 'rm -rf "$workdir"' EXIT

openssl s_client \
  -servername "$issuer_host" \
  -showcerts \
  -connect "$issuer_host:443" </dev/null 2>/dev/null |
  awk '
    /BEGIN CERTIFICATE/ { n++; file=sprintf("'"$workdir"'/cert%d.pem", n) }
    file { print > file }
    /END CERTIFICATE/ { file="" }
  '

first_cert="$workdir/cert1.pem"
if [ ! -s "$first_cert" ]; then
  echo "Failed to read certificate chain for $issuer_host" >&2
  exit 1
fi

thumbprint=$(openssl x509 \
  -in "$first_cert" \
  -fingerprint \
  -sha1 \
  -noout |
  sed 's/.*=//;s/://g' |
  tr 'A-Z' 'a-z')

echo "Creating EKS OIDC provider for $issuer"
aws iam create-open-id-connect-provider \
  --url "$issuer" \
  --client-id-list sts.amazonaws.com \
  --thumbprint-list "$thumbprint" >/dev/null
