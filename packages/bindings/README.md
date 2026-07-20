# `@alienplatform/bindings`

Direct TypeScript bindings for Alien storage, kv, queue, and vault over an
in-process [napi-rs](https://napi.rs) addon. The addon itself lives in the Rust
crate `crates/alien-bindings-node`; this package is the published JavaScript
wrapper that loads it.

## Remote Storage

Use `Bindings.forRemoteDeployment` from a trusted backend to access a Storage
resource in an existing deployment. The token must be authorized for remote
bindings on that deployment.

```ts
import { Bindings } from "@alienplatform/bindings"

const bindings = await Bindings.forRemoteDeployment({
  deploymentId: process.env.ALIEN_DEPLOYMENT_ID!,
  token: process.env.ALIEN_API_TOKEN!,
})

const archive = bindings.storage("archive")
await archive.put("reports/latest.json", Buffer.from(JSON.stringify({ ready: true })))

const metadata = await archive.head("reports/latest.json")
const report = await archive.get("reports/latest.json")
const reports = await archive.list("reports/")

await archive.delete("reports/latest.json")
```

Remote Storage exposes `get`, `put`, `head`, `list`, and `delete`. It does not
expose copy or signed URLs. The same `Bindings` and Storage handles remain valid
while the native client refreshes short-lived cloud credentials. Pass
`apiBaseUrl` only when targeting a non-default Alien API endpoint.

## Native addon resolution

The addon is loaded lazily on the first binding operation (never at import — the
package is `sideEffects: false`). `src/loader.ts` resolves it in order:

1. `ALIEN_BINDINGS_ADDON_PATH` — an explicit path to a `.node` file. A dev/test
   escape hatch only; never set in a published install.
2. The per-platform prebuild package `@alienplatform/bindings-<triple>`, pulled
   in as an `optionalDependency`. This is how end users get the addon: `npm`/`bun`
   installs only the package matching the host `os`/`cpu`/`libc`. The
   `optionalDependencies` block exists **only in the published manifest** — it is
   injected at publish time by the release pipeline, so a workspace checkout
   carries none.
3. Dev fallback: a locally-built addon at
   `crates/alien-bindings-node/alien-bindings-node.<triple>.node`, found by
   walking up from the installed package. Loaded only if its `version()` matches
   this package's version (a stale build is warned about and rejected).

## Local development

Build the addon for your host once, then run anything that imports the package:

```sh
bun run build:addon   # or: pnpm -C packages/bindings run build:addon
```

`build:addon` runs `napi build --platform --release` against
`crates/alien-bindings-node` and drops the `.node` next to the crate, where the
loader's dev fallback (step 3 above) finds it. Rebuild after changing any Rust
in `alien-bindings` or `alien-bindings-node`. The built `.node` is gitignored.
`build:addon` only builds for the host triple; on a Mac, cross-building the
other mac triple (e.g. `darwin-x64` from an `arm64` host) needs an explicit
`napi build --release --target x86_64-apple-darwin --cwd
../../crates/alien-bindings-node`.

To point the loader at an addon somewhere else, set `ALIEN_BINDINGS_ADDON_PATH`
to its path (step 1).

## Prebuild packages (`npm/`)

`npm/<triple>/` holds the skeleton for each published per-platform package
(`darwin-arm64`, `darwin-x64`, `linux-x64-gnu`, `linux-arm64-gnu`). Each carries
a `package.json` (`name`/`os`/`cpu`/`libc`/`main`/`files`) and a README; the
`.node` is staged in at build time and is never committed. There is no musl
target: the deployment base images are glibc (chainguard/wolfi-base), not Alpine.

The release pipeline builds each addon on its native runner, stages it into the
matching `npm/<triple>/` dir, rewrites the placeholder `0.0.0` versions to the
release version, injects the exact-version `optionalDependencies` into this
package's published manifest, and publishes the platform packages before the
wrapper. Pinning the exact version is what guarantees a published wrapper only
ever loads the matching-version platform addon.
