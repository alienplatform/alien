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
# The providers below must match PROVIDER_REQUIREMENTS in crates/alien-terraform/src/generator.rs,
# line for line and in order; the `provider_mirror_matches_generator_requirements` test fails
# otherwise. A provider that isn't mirrored still installs from the registry.
set -euo pipefail

mirror="${1:?usage: $0 <mirror-dir> <cli-config-file>}"
config="${2:?usage: $0 <mirror-dir> <cli-config-file>}"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

cat > "$work/main.tf" <<'TF'
terraform {
  required_providers {
    aws         = { source = "hashicorp/aws", version = ">= 6.70" }
    awscc       = { source = "hashicorp/awscc", version = ">= 1.0" }
    tls         = { source = "hashicorp/tls", version = ">= 4.0" }
    google      = { source = "hashicorp/google", version = ">= 7.37" }
    google-beta = { source = "hashicorp/google-beta", version = ">= 6.0" }
    azurerm     = { source = "hashicorp/azurerm", version = ">= 4.75, < 5.0" }
    azapi       = { source = "Azure/azapi", version = ">= 2.6, < 3.0" }
    time        = { source = "hashicorp/time", version = ">= 0.9" }
    kubernetes  = { source = "hashicorp/kubernetes", version = ">= 2.30" }
    helm        = { source = "hashicorp/helm", version = ">= 3.0" }
    random      = { source = "hashicorp/random", version = ">= 3.7.2" }
  }
}
TF

terraform -chdir="$work" init -backend=false -input=false >/dev/null
mkdir -p "$mirror"
cp -R "$work/.terraform/providers/." "$mirror/"

# Tests that check the oldest provider versions the generated modules still accept pin them
# exactly (crates/alien-terraform/tests/emitter_postgres.rs and its credential-model fixture),
# so those versions are mirrored next to the newest ones. Without them the init finds no match,
# because a mirrored provider is never fetched from the registry.
minimum="$work/minimum"
mkdir -p "$minimum"
cat > "$minimum/main.tf" <<'TF'
terraform {
  required_providers {
    aws     = { source="hashicorp/aws", version="= 6.70.0" }
    google  = { source="hashicorp/google", version="= 7.37.0" }
    azurerm = { source="hashicorp/azurerm", version="= 4.75.0" }
    random  = { source="hashicorp/random", version="= 3.7.2" }
  }
}
TF
terraform -chdir="$minimum" init -backend=false -input=false >/dev/null
cp -R "$minimum/.terraform/providers/." "$mirror/"

# The Azure Postgres cloud-free validation test pins azurerm 4.81.0 exactly.
azure_pin="$work/azure-pin"
mkdir -p "$azure_pin"
cat > "$azure_pin/main.tf" <<'TF'
terraform {
  required_providers {
    azurerm = { source="hashicorp/azurerm", version="= 4.81.0" }
  }
}
TF
terraform -chdir="$azure_pin" init -backend=false -input=false >/dev/null
cp -R "$azure_pin/.terraform/providers/." "$mirror/"

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
