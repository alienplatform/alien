#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
sdk_dir="${repo_root}/client-sdks/manager/typescript"
schema="${repo_root}/client-sdks/manager/openapi.json"
# The checked-in workflow pins the generator version. Current open-source
# Speakeasy CLIs honor that pin and fetch the matching generator when needed.
command -v speakeasy >/dev/null 2>&1 || {
  echo "Speakeasy CLI is required; install it through the workspace bootstrap." >&2
  exit 1
}

sdk_version="$(node -p "require('${sdk_dir}/package.json').version")"

speakeasy lint openapi --schema "${schema}"
(
  cd "${sdk_dir}"
  speakeasy run \
    --target manager-typescript \
    --set-version "${sdk_version}" \
    --skip-versioning \
    --skip-testing \
    --skip-upload-spec \
    --output console
)

# Speakeasy currently emits whitespace-only indentation and extra blank lines at
# EOF in some generated Markdown. Keep this narrow: normalize only files that
# this run added or changed, including untracked generated Markdown.
while IFS= read -r -d '' markdown_file; do
  [[ -f "${sdk_dir}/${markdown_file}" ]] || continue
  env LC_ALL=C perl -0pi -e 's/[ \t]+$//mg; s/\n*\z/\n/' "${sdk_dir}/${markdown_file}"
done < <(
  {
    git -C "${sdk_dir}" diff --relative --name-only -z -- '*.md'
    git -C "${sdk_dir}" ls-files --others --exclude-standard -z -- '*.md'
  } | sort -zu
)

if ! git -C "${sdk_dir}" diff --check -- .; then
  echo "Generated Manager SDK contains whitespace errors." >&2
  exit 1
fi

generated_npm_version="$(node -p "require('${sdk_dir}/package.json').version")"
generated_jsr_version="$(node -p "require('${sdk_dir}/jsr.json').version")"
generated_npm_lock_version="$(node -p "require('${sdk_dir}/package-lock.json').version")"
generated_examples_lock_version="$(node -p "require('${sdk_dir}/examples/package-lock.json').packages['..'].version")"
if [[ "${generated_npm_version}" != "${sdk_version}" \
  || "${generated_jsr_version}" != "${sdk_version}" \
  || "${generated_npm_lock_version}" != "${sdk_version}" \
  || "${generated_examples_lock_version}" != "${sdk_version}" ]]; then
  echo "Generated SDK version drifted from ${sdk_version}: npm=${generated_npm_version}, jsr=${generated_jsr_version}, npm-lock=${generated_npm_lock_version}, examples-lock=${generated_examples_lock_version}." >&2
  exit 1
fi

pnpm -C "${sdk_dir}" build
