#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
sdk_dir="${repo_root}/client-sdks/platform/typescript"
schema="${repo_root}/client-sdks/platform/openapi.json"

# The checked-in workflow pins the generator version. Current open-source
# Speakeasy CLIs honor that pin and fetch the matching generator when needed.
command -v speakeasy >/dev/null 2>&1 || {
  echo "Speakeasy CLI is required." >&2
  exit 1
}

sdk_version="$(node -p "require('${sdk_dir}/package.json').version")"

speakeasy lint openapi --schema "${schema}"
(
  cd "${sdk_dir}"
  speakeasy run \
    --target platform-typescript \
    --set-version "${sdk_version}" \
    --skip-versioning \
    --skip-testing \
    --skip-compile \
    --skip-upload-spec \
    --output console
)

# Keep emitted declarations rooted under src. Isolated npm release builds use
# TypeScript's package-local dependency graph and require this boundary.
env LC_ALL=C perl -0pi -e \
  's/^    "rootDir": "src",\n//mg; s/(^    "sourceMap": true,\n)/$1    "rootDir": "src",\n/m' \
  "${sdk_dir}/tsconfig.json"
if ! grep -q '^    "rootDir": "src",$' "${sdk_dir}/tsconfig.json"; then
  echo "Failed to preserve the Platform SDK TypeScript rootDir." >&2
  exit 1
fi

# Speakeasy can emit trailing whitespace in generated Markdown. Normalize only
# files added or changed by this generation run.
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
  echo "Generated Platform SDK contains whitespace errors." >&2
  exit 1
fi

# Speakeasy does not currently update the npm lock's embedded package version.
# Keep it aligned with the generated publish manifests.
node -e "
  const fs = require('fs');
  const path = '${sdk_dir}/package-lock.json';
  const lock = JSON.parse(fs.readFileSync(path, 'utf8'));
  lock.version = '${sdk_version}';
  lock.packages[''].version = '${sdk_version}';
  fs.writeFileSync(path, JSON.stringify(lock, null, 2) + '\\n');
"

generated_npm_version="$(node -p "require('${sdk_dir}/package.json').version")"
generated_jsr_version="$(node -p "require('${sdk_dir}/jsr.json').version")"
generated_npm_lock_version="$(node -p "require('${sdk_dir}/package-lock.json').version")"
generated_npm_lock_package_version="$(node -p "require('${sdk_dir}/package-lock.json').packages[''].version")"
generated_workflow_version="$(node -e "
  const fs = require('fs');
  const contents = fs.readFileSync('${sdk_dir}/.speakeasy/gen.yaml', 'utf8');
  const match = contents.match(/^typescript:\\n  version: ([^\\n]+)$/m);
  if (!match) throw new Error('Platform SDK version not found in gen.yaml');
  process.stdout.write(match[1]);
")"
generated_lock_version="$(node -e "
  const fs = require('fs');
  const contents = fs.readFileSync('${sdk_dir}/.speakeasy/gen.lock', 'utf8');
  const match = contents.match(/^  releaseVersion: ([^\\n]+)$/m);
  if (!match) throw new Error('Platform SDK releaseVersion not found in gen.lock');
  process.stdout.write(match[1]);
")"
if [[ "${generated_npm_version}" != "${sdk_version}" \
  || "${generated_jsr_version}" != "${sdk_version}" \
  || "${generated_npm_lock_version}" != "${sdk_version}" \
  || "${generated_npm_lock_package_version}" != "${sdk_version}" \
  || "${generated_workflow_version}" != "${sdk_version}" \
  || "${generated_lock_version}" != "${sdk_version}" ]]; then
  echo "Generated SDK version drifted from ${sdk_version}: npm=${generated_npm_version}, jsr=${generated_jsr_version}, npm-lock=${generated_npm_lock_version}, npm-lock-package=${generated_npm_lock_package_version}, workflow=${generated_workflow_version}, generator-lock=${generated_lock_version}." >&2
  exit 1
fi

NODE_OPTIONS=--max-old-space-size=8192 pnpm -C "${sdk_dir}" build
