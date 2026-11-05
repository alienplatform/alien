#!/usr/bin/env bash
# Release signing on Kubernetes, in a disposable kind cluster: the chart's signing ConfigMaps
# survive `helm uninstall`, the Operator can't rewrite its settings (R14), values can't pick
# another Operator build (R12), and air-gapped sync refuses a foreign chart on a protected
# site (R13). Every attack also runs unprotected, where it succeeds.
#
# Usage: scripts/helm-signing-kind/run.sh
#   KIND_CLUSTER   cluster name (default helm-signing-$$); deleted on exit
#   OUT            keep the test output here
set -euo pipefail

repo=$(git -C "$(dirname "$0")" rev-parse --show-toplevel)
cluster=${KIND_CLUSTER:-helm-signing-$$}
work=$(mktemp -d)
out=${OUT:-$work}
kubeconfig="$work/kubeconfig"

cleanup() {
  kind delete cluster --name "$cluster" >/dev/null 2>&1 || true
  rm -rf "$work"
}
trap cleanup EXIT

kind create cluster --name "$cluster" --kubeconfig "$kubeconfig" --wait 120s

cd "$repo"
KUBECONFIG="$kubeconfig" ALIEN_HELM_TEST_KUBECONFIG="$kubeconfig" \
  cargo test -p alien-helm --test signing_kind -- --ignored --test-threads=1 \
  2>&1 | tee "$out/alien-helm.log"
KUBECONFIG="$kubeconfig" ALIEN_SYNC_TEST_CONTEXT="kind-$cluster" \
  cargo test -p alien-deploy-cli --lib -- --ignored --exact --test-threads=1 \
  commands::sync::tests::protected_site_refuses_a_foreign_chart_from_chart_defaults \
  commands::sync::tests::bundle_strings_cannot_assign_other_values \
  2>&1 | tee "$out/alien-deploy-cli.log"
