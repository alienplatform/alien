/**
 * Native addon loader.
 *
 * Resolves the napi-rs addon for the current platform, in order:
 *
 *   1. `ALIEN_BINDINGS_ADDON_PATH` — an explicit path to a `.node` file. This is
 *      a dev/test-only escape hatch and is never set in published installs.
 *   2. The per-platform prebuild package from `optionalDependencies`
 *      (`@alienplatform/bindings-<triple>`) — how end users get the addon.
 *      `optionalDependencies` only exists in the *published* manifest: the
 *      release pipeline injects the exact-version package list with
 *      `scripts/inject-optional-deps.mjs`. The workspace source manifest carries
 *      no `optionalDependencies`, so this path is a no-op
 *      (module-not-found) in every dev/test checkout — expected, and why step 3
 *      exists below. Its reported version must match this wrapper package;
 *      mixed wrapper/prebuild versions are rejected before any binding runs.
 *   3. Dev fallback: the locally-built addon under
 *      `crates/alien-bindings-node/alien-bindings-node.<triple>.node`, located by
 *      walking up from this module. This lets the repo run against a
 *      `napi build`-produced `.node` without publishing a prebuild. Before
 *      trusting this addon, its `version()` is compared against this
 *      package's own `package.json` version; a mismatch (a stale local build
 *      left over from an earlier checkout) is logged and rejected rather than
 *      loaded, falling through to the same "cannot load" error as if no addon
 *      had been found at all.
 *
 * Loading is deferred: `loadAddon()` is only called on the first binding
 * operation (see `factories.ts`), never at module import. That is what makes the
 * package safe to mark `sideEffects: false` — importing it performs no I/O and
 * requires no addon.
 */

import { existsSync } from "node:fs"
import { createRequire } from "node:module"
import { dirname, join } from "node:path"
import { fileURLToPath } from "node:url"

const require = createRequire(import.meta.url)

/** Raw napi scan page. */
export interface RawKvItem {
  key: string
  value: Buffer
}

/** Raw napi scan result. */
export interface RawScanResult {
  items: RawKvItem[]
  nextCursor?: string
}

/** Raw napi object metadata. */
export interface RawObjectMeta {
  location: string
  size: number
  lastModified: string
}

/** Raw napi presigned request. */
export interface RawPresignedRequest {
  url: string
  method: string
  headers: Record<string, string>
}

/** Raw napi queue message. */
export interface RawQueueMessage {
  payloadType: "json" | "text"
  payload: string
  receiptHandle: string
  attempt: number
}

/** Raw napi storage handle. */
export interface RawStorageHandle {
  get(path: string): Promise<Buffer>
  put(path: string, data: Buffer): Promise<void>
  delete(path: string): Promise<void>
  list(prefix?: string | null): Promise<RawObjectMeta[]>
  head(path: string): Promise<RawObjectMeta>
  copy(from: string, to: string): Promise<void>
  signedUrl(method: string, path: string, expiresInSecs: number): Promise<RawPresignedRequest>
}

/** Raw napi key-value handle. */
export interface RawKvHandle {
  get(key: string): Promise<Buffer | null>
  put(
    key: string,
    value: Buffer,
    ttlSecs?: number | null,
    ifNotExists?: boolean | null,
  ): Promise<boolean>
  delete(key: string): Promise<void>
  exists(key: string): Promise<boolean>
  scan(prefix: string, limit?: number | null, cursor?: string | null): Promise<RawScanResult>
}

/** Raw napi queue handle. Every method takes the queue name as its first arg. */
export interface RawQueueHandle {
  sendJson(queue: string, jsonString: string): Promise<void>
  sendText(queue: string, text: string): Promise<void>
  receive(queue: string, max: number): Promise<RawQueueMessage[]>
  ack(queue: string, receipt: string): Promise<void>
  nack(queue: string, receipt: string): Promise<void>
  purge(queue: string): Promise<void>
}

/** Raw napi vault handle. */
export interface RawVaultHandle {
  getSecret(name: string): Promise<string>
  setSecret(name: string, value: string): Promise<void>
  deleteSecret(name: string): Promise<void>
  listSecrets(): Promise<string[]>
}

/** Raw napi bindings entry point. Construction validates the environment. */
export interface RawBindingsHandle {
  storage(name: string): Promise<RawStorageHandle>
  kv(name: string): Promise<RawKvHandle>
  queue(name: string): Promise<RawQueueHandle>
  vault(name: string): Promise<RawVaultHandle>
}

/** The complete napi addon module surface consumed by the wrapper. */
export interface NativeAddon {
  BindingsHandle: new () => RawBindingsHandle
  version(): string
}

/** The Linux C library a prebuild is compiled against. */
export type LinuxLibc = "gnu" | "musl"

/**
 * Detect the Linux C library (glibc vs musl) of the current process, using the
 * same signal as napi-rs's generated loader: a glibc runtime reports a
 * `glibcVersionRuntime` in its process report; a musl runtime (Alpine, etc.)
 * does not. Falls back to the Alpine marker file when the report is
 * unavailable.
 *
 * Only meaningful on Linux — `platformTriple` consults the result solely on the
 * `linux` branch, so the value returned on other platforms is unused.
 */
export function detectLinuxLibc(): LinuxLibc {
  const report =
    typeof process.report?.getReport === "function"
      ? (process.report.getReport() as { header?: { glibcVersionRuntime?: string } })
      : undefined
  if (report?.header) {
    // glibc runtimes populate this field; musl runtimes leave it absent.
    return report.header.glibcVersionRuntime ? "gnu" : "musl"
  }
  // Process report unavailable: Alpine ships this marker and has no glibc.
  return existsSync("/etc/alpine-release") ? "musl" : "gnu"
}

/**
 * Map `process.platform` / `process.arch` to the napi triple used in both the
 * prebuild package name and the locally-built `.node` file name. Mirrors the
 * `optionalDependencies` set pinned in PACKAGE_LAYOUT.md.
 *
 * Only glibc Linux prebuilds are published (`…-gnu`; see PACKAGE_LAYOUT.md), so
 * a musl host (Alpine and friends) has no addon: this throws a clear
 * unsupported-platform error naming musl rather than silently selecting the
 * glibc triple, which would otherwise fail to resolve — or, worse, load a glibc
 * `.node` into a musl process and crash on the first binding call.
 *
 * Exported for unit testing: accepts `platform`/`arch`/`libc` explicitly
 * (defaulting to the detected values) so every supported pair — and the
 * unsupported cases — can be exercised directly, without stubbing `process`.
 */
export function platformTriple(
  platform: NodeJS.Platform = process.platform,
  arch: NodeJS.Architecture = process.arch,
  libc: LinuxLibc = platform === "linux" ? detectLinuxLibc() : "gnu",
): string {
  if (platform === "darwin" && arch === "arm64") return "darwin-arm64"
  if (platform === "darwin" && arch === "x64") return "darwin-x64"
  if (platform === "linux" && libc === "musl") {
    throw new Error(
      `@alienplatform/bindings has no native addon for musl-based Linux (arch '${arch}'). Prebuilds are published for glibc Linux only (the '…-gnu' triples); run on a glibc-based image (for example debian- or ubuntu-slim) instead.`,
    )
  }
  if (platform === "linux" && arch === "x64") return "linux-x64-gnu"
  if (platform === "linux" && arch === "arm64") return "linux-arm64-gnu"
  throw new Error(
    `@alienplatform/bindings has no native addon for platform '${platform}' arch '${arch}'.`,
  )
}

/**
 * Walk up from `startDir` (default: this module's directory) looking for the
 * locally-built addon under `crates/alien-bindings-node`, returning its path or
 * `undefined` if the walk reaches the filesystem root without finding it.
 * Repo-internal dev/test only. Also reused by `scripts/compile-smoke.ts`.
 */
export function findLocalAddon(
  triple: string,
  startDir: string = dirname(fileURLToPath(import.meta.url)),
): string | undefined {
  const fileName = `alien-bindings-node.${triple}.node`
  let dir = startDir
  // Bounded walk to the filesystem root.
  for (;;) {
    const candidate = join(dir, "crates", "alien-bindings-node", fileName)
    if (existsSync(candidate)) return candidate
    const parent = dirname(dir)
    if (parent === dir) return undefined
    dir = parent
  }
}

/** This package's own `version` field, read from `package.json` at the package root. */
function packageVersion(): string {
  const dir = dirname(fileURLToPath(import.meta.url))
  const packageJson = require(join(dir, "..", "package.json")) as { version: string }
  return packageJson.version
}

/**
 * Reject wrapper/addon version skew before the native module is used.
 *
 * The wrapper and every platform prebuild are published as one release. npm
 * normally installs the exact version pinned in `optionalDependencies`, but a
 * stale lockfile, package-manager override, or copied `node_modules` can still
 * leave a different native binary on disk. Loading that binary risks calling
 * an incompatible napi surface, so fail with the two observed versions.
 */
export function assertAddonVersion(addon: NativeAddon, expected: string, source: string): void {
  const actual = addon.version()
  if (actual !== expected) {
    throw new Error(
      `@alienplatform/bindings native addon version mismatch for ${source}: addon reports '${actual}', wrapper is '${expected}'. Reinstall @alienplatform/bindings so the wrapper and platform prebuild use the same version.`,
    )
  }
}

let cached: NativeAddon | undefined
let embedded: NativeAddon | undefined

/**
 * Register an addon that is already resident in the process — the one bun
 * embeds into a `bun build --compile` binary through the `./native` entry.
 *
 * A compiled workload has no filesystem prebuild and no dev checkout to walk,
 * so none of the resolution steps in {@link loadAddon} can find the addon. The
 * build makes the compiled entry import `@alienplatform/bindings/native`, whose
 * `installEmbeddedAddon()` calls this so the ordinary `@alienplatform/bindings`
 * factories (which go through {@link loadAddon}) use the embedded addon. In a
 * non-compiled dev/test run nothing imports `./native`, so this is never called
 * and {@link loadAddon} falls through to its normal resolution.
 */
export function registerEmbeddedAddon(addon: NativeAddon): void {
  embedded = addon
}

/**
 * Load (and memoize) the native addon. Throws if no addon can be resolved for
 * the current platform. Called lazily on the first binding operation.
 */
export function loadAddon(): NativeAddon {
  if (cached) return cached

  // A compiled binary registers its embedded addon up front; prefer it over the
  // filesystem/prebuild resolution below, which cannot work inside the binary.
  if (embedded) {
    cached = embedded
    return cached
  }

  const override = process.env.ALIEN_BINDINGS_ADDON_PATH
  if (override) {
    cached = require(override) as NativeAddon
    return cached
  }

  const triple = platformTriple()
  const pkg = `@alienplatform/bindings-${triple}`

  let publishedPath: string | undefined
  try {
    publishedPath = require.resolve(pkg)
  } catch {
    // Prebuild not installed — fall through to the dev-built addon.
  }
  if (publishedPath) {
    const addon = require(publishedPath) as NativeAddon
    assertAddonVersion(addon, packageVersion(), `published prebuild '${pkg}'`)
    cached = addon
    return cached
  }

  const local = findLocalAddon(triple)
  if (local) {
    const addon = require(local) as NativeAddon
    const expected = packageVersion()
    // Trust the local addon only when its reported version matches the installed
    // package version. A mismatch means a stale build left over from an earlier
    // checkout (ABI/version skew) and must not be loaded.
    try {
      assertAddonVersion(addon, expected, `locally-built addon at '${local}'`)
      cached = addon
      return cached
    } catch (error) {
      // Stale locally-built addon (ABI/version skew) — warn and fall through to
      // the standard "cannot load" error below rather than serving a mismatched
      // binary. A dev checkout can repair this by rebuilding locally, while a
      // published prebuild mismatch above is a broken installation and fails
      // immediately with reinstall guidance.
      console.warn(
        `${error instanceof Error ? error.message : String(error)} Rebuild it with \`napi build --platform\` in crates/alien-bindings-node.`,
      )
    }
  }

  throw new Error(
    `Cannot load the @alienplatform/bindings native addon for '${triple}'. Install the '${pkg}' prebuild, or build it locally with \`napi build --platform\` in crates/alien-bindings-node, or set ALIEN_BINDINGS_ADDON_PATH to a built .node file.`,
  )
}
