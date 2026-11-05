#!/usr/bin/env bash
# Lint every protected setup template the CLI renders: cfn-lint on the CloudFormation template,
# terraform init + validate on each Terraform module, helm lint on the chart.
#
# The templates come from the alien-cli-common setup_templates test, which writes what it
# renders into $SETUP_TEMPLATES_OUT. Needs cargo, cfn-lint, terraform (with network access for
# `terraform init`) and helm.
#
#   scripts/setup-render-diff/lint-protected.sh [<out-dir>]
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
out="${1:-$(mktemp -d)}"
mkdir -p "$out"

SETUP_TEMPLATES_OUT="$out" cargo test -p alien-cli-common --test setup_templates \
  renders_every_setup_template_the_same_way_twice --manifest-path "$root/Cargo.toml" -q

for name in cloudformation terraform-aws terraform-gcp terraform-azure helm; do
  test -s "$out/$name" || { echo "missing rendered template $name" >&2; exit 1; }
done

echo "== cfn-lint cloudformation"
cp "$out/cloudformation" "$out/cloudformation.yaml"
# Exit code 4 means warnings only (an unused parameter, a DependsOn a Ref already implies: the
# same on unprotected templates); anything with an error bit fails.
status=0
cfn-lint "$out/cloudformation.yaml" || status=$?
if [ "$status" -ne 0 ] && [ "$status" -ne 4 ]; then
  echo "cfn-lint found errors (exit $status)" >&2
  exit 1
fi

# Terraform and Helm templates are JSON objects mapping a file path to its contents.
unpack() {
  python3 - "$1" "$2" <<'EOF'
import json, pathlib, sys
files = json.loads(pathlib.Path(sys.argv[1]).read_text())
for path, contents in files.items():
    target = pathlib.Path(sys.argv[2]) / path
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(contents)
EOF
}

for name in terraform-aws terraform-gcp terraform-azure; do
  echo "== terraform validate $name"
  dir="$out/$name.d"
  rm -rf "$dir"
  unpack "$out/$name" "$dir"
  terraform -chdir="$dir" init -backend=false -input=false -no-color >/dev/null
  terraform -chdir="$dir" validate -no-color
done

echo "== helm lint"
rm -rf "$out/helm.d"
unpack "$out/helm" "$out/helm.d"
chart="$(dirname "$(find "$out/helm.d" -name Chart.yaml | head -1)")"
helm lint "$chart"

echo "all protected setup templates lint clean ($out)"
