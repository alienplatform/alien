#!/usr/bin/env bash

set -euo pipefail

readonly max_lines=2000
readonly repo_root="$(git rev-parse --show-toplevel)"

cd "$repo_root"

source_pathspecs=(
  ":(glob)*.rs"
  ":(glob)**/*.rs"
  ":(glob)*.ts"
  ":(glob)**/*.ts"
  ":(glob)*.tsx"
  ":(glob)**/*.tsx"
  ":(glob)*.js"
  ":(glob)**/*.js"
  ":(glob)*.jsx"
  ":(glob)**/*.jsx"
  ":(exclude,glob)client-sdks/**"
  ":(exclude,glob)packages/sdk/src/worker-runtime/generated/**"

  # The #[controller] macro requires the annotated struct and its single
  # annotated impl block to live in one module.
  ":(exclude)crates/alien-infra/src/worker/gcp/mod.rs"
  ":(exclude)crates/alien-infra/src/worker/aws/mod.rs"
  ":(exclude)crates/alien-infra/src/worker/azure/mod.rs"

  # Pre-existing large files pending follow-up splits.
  ":(exclude)crates/alien-deploy-cli/src/commands/join.rs"
  ":(exclude)crates/alien-infra/src/kubernetes_public_endpoint.rs"
  ":(exclude)crates/alien-infra/src/network/aws.rs"
  ":(exclude)crates/alien-bindings/src/provider.rs"
  ":(exclude)crates/alien-bindings/tests/storage.rs"
  ":(exclude)crates/alien-infra/src/container/kubernetes.rs"
  ":(exclude)crates/alien-infra/src/network/azure.rs"
  ":(exclude)crates/alien-infra/src/core/executor.rs"
  ":(exclude)crates/alien-commands/src/server/mod.rs"
  ":(exclude)crates/alien-cli/src/commands/release.rs"
  ":(exclude)crates/alien-preflights/src/mutations/compute_cluster.rs"
)

git ls-files -z -- "${source_pathspecs[@]}" |
  xargs -0 wc -l |
  awk -v max_lines="$max_lines" '
    $2 != "total" && $1 > max_lines {
      line_count = $1
      sub(/^[[:space:]]*[0-9]+[[:space:]]+/, "", $0)
      printf "%s has %s lines (max %s) - split this file or add an explicit exclusion\n",
        $0, line_count, max_lines
      found = 1
    }
    END { exit found }
  '
