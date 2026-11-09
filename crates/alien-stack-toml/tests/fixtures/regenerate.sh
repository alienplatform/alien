#!/usr/bin/env bash
# Regenerates each fixture's expected.json by evaluating its alien.ts with the TypeScript
# SDK in this repository (packages/core/src), the way `alien build` evaluates alien.ts.
#
# The differential test (tests/differential.rs) then requires the fixture's alien.toml to
# resolve to the same stack. Run this after changing the TypeScript SDK or a fixture.
# Requires Bun and `pnpm install` in the repository.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$here/../../../.." && pwd)"
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT

# Resolve `@alienplatform/core` to the SDK source rather than a published or built copy.
package="$scratch/node_modules/@alienplatform/core"
mkdir -p "$package"
printf '{ "name": "@alienplatform/core", "type": "module", "exports": "./index.ts" }\n' >"$package/package.json"
printf 'export * from "%s/packages/core/src/index.ts"\n' "$repo" >"$package/index.ts"

# With arguments, only those fixtures are regenerated: regenerate.sh pools sandbox-runtime
if [ "$#" -gt 0 ]; then
  fixtures=()
  for name in "$@"; do fixtures+=("$here/$name/"); done
else
  fixtures=("$here"/*/)
fi

for fixture in "${fixtures[@]}"; do
  name="$(basename "$fixture")"
  cp "$fixture/alien.ts" "$scratch/$name.ts"
  (cd "$scratch" && bun -e "const config = await import('./$name.ts'); console.log(JSON.stringify(config.default, null, 2))") \
    >"$fixture/expected.json"
  echo "regenerated $name/expected.json"
done
