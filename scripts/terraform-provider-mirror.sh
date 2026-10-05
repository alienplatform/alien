#!/usr/bin/env bash
# Builds a read-only Terraform provider mirror for the Terraform generator tests.
#
# Every `terraform_validate` in crates/alien-terraform/src/test_utils.rs runs `terraform init`
# in a fresh directory, which downloads each provider again. A shared TF_PLUGIN_CACHE_DIR is not
# safe under parallel inits (they write the cache and fail checksum checks), but a filesystem
# mirror is only ever read.
#
# Usage: scripts/terraform-provider-mirror.sh <mirror-dir> <cli-config-file>
# Then export TF_CLI_CONFIG_FILE=<cli-config-file>.
#
# Keep the providers below in sync with `required_providers` in
# crates/alien-terraform/src/generator.rs. A provider missing here still installs from the
# registry, because the config only routes the mirrored providers to the mirror.
set -euo pipefail

mirror="${1:?usage: $0 <mirror-dir> <cli-config-file>}"
config="${2:?usage: $0 <mirror-dir> <cli-config-file>}"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

cat > "$work/main.tf" <<'TF'
terraform {
  required_providers {
    aws         = { source = "hashicorp/aws", version = ">= 5.0" }
    awscc       = { source = "hashicorp/awscc", version = ">= 1.0" }
    tls         = { source = "hashicorp/tls", version = ">= 4.0" }
    google      = { source = "hashicorp/google", version = ">= 5.0" }
    google-beta = { source = "hashicorp/google-beta", version = ">= 6.0" }
    azurerm     = { source = "hashicorp/azurerm", version = ">= 4.75, < 5.0" }
    azapi       = { source = "Azure/azapi", version = ">= 2.6, < 3.0" }
    time        = { source = "hashicorp/time", version = ">= 0.9" }
    kubernetes  = { source = "hashicorp/kubernetes", version = ">= 2.30" }
    helm        = { source = "hashicorp/helm", version = ">= 3.0" }
    random      = { source = "hashicorp/random", version = ">= 3.6" }
  }
}
TF

terraform -chdir="$work" init -backend=false -input=false >/dev/null
mkdir -p "$mirror"
cp -R "$work/.terraform/providers/." "$mirror/"

# Route exactly the mirrored providers (host/namespace/type) to the mirror.
providers="$(cd "$mirror" && find . -mindepth 3 -maxdepth 3 -type d | sed 's|^\./||' | sort)"
list="$(printf '%s\n' "$providers" | awk 'NF { printf "%s\"%s\"", (n++ ? ", " : ""), $0 }')"

cat > "$config" <<RC
provider_installation {
  filesystem_mirror {
    path    = "$mirror"
    include = [$list]
  }
  direct {
    exclude = [$list]
  }
}
RC
echo "Mirrored $(printf '%s\n' "$providers" | grep -c .) providers into $mirror"
