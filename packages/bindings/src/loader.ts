/**
 * Native addon loader.
 *
 * Resolves the napi-rs addon for the current platform, in order:
 *
 *   1. `ALIEN_BINDINGS_ADDON_PATH` — an explicit path to a `.node` file. This is
 *      a dev/test-only escape hatch and is never set in published installs.
 *   2. The per-platform prebuild package from `optionalDependencies`
 *      (`@alienplatform/bindings-<triple>`) — how end users get the addon.
 *   3. Dev fallback: the locally-built addon under
 *      `crates/alien-bindings-node/alien-bindings-node.<triple>.node`, located by
 *      walking up from this module. This lets the repo run against a
 *      `napi build`-produced `.node` without publishing a prebuild.
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
  payloadType: string
  payloadJson?: string
  payloadText?: string
  receiptHandle: string
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
  BindingsHandle: new (envOverride?: Record<string, string> | null) => RawBindingsHandle
  version(): string
}

/**
 * Map `process.platform` / `process.arch` to the napi triple used in both the
 * prebuild package name and the locally-built `.node` file name. Mirrors the
 * `optionalDependencies` set pinned in PACKAGE_LAYOUT.md.
 */
function platformTriple(): string {
  const { platform, arch } = process
  if (platform === "darwin" && arch === "arm64") return "darwin-arm64"
  if (platform === "linux" && arch === "x64") return "linux-x64-gnu"
  if (platform === "linux" && arch === "arm64") return "linux-arm64-gnu"
  throw new Error(
    `@alienplatform/bindings has no native addon for platform '${platform}' arch '${arch}'.`,
  )
}

/**
 * Walk up from this module looking for the locally-built addon under
 * `crates/alien-bindings-node`. Repo-internal dev/test only.
 */
function findLocalAddon(triple: string): string | undefined {
  const fileName = `alien-bindings-node.${triple}.node`
  let dir = dirname(fileURLToPath(import.meta.url))
  // Bounded walk to the filesystem root.
  for (;;) {
    const candidate = join(dir, "crates", "alien-bindings-node", fileName)
    if (existsSync(candidate)) return candidate
    const parent = dirname(dir)
    if (parent === dir) return undefined
    dir = parent
  }
}

let cached: NativeAddon | undefined

/**
 * Load (and memoize) the native addon. Throws if no addon can be resolved for
 * the current platform. Called lazily on the first binding operation.
 */
export function loadAddon(): NativeAddon {
  if (cached) return cached

  const override = process.env.ALIEN_BINDINGS_ADDON_PATH
  if (override) {
    cached = require(override) as NativeAddon
    return cached
  }

  const triple = platformTriple()
  const pkg = `@alienplatform/bindings-${triple}`

  try {
    cached = require(pkg) as NativeAddon
    return cached
  } catch {
    // Prebuild not installed — fall through to the dev-built addon.
  }

  const local = findLocalAddon(triple)
  if (local) {
    cached = require(local) as NativeAddon
    return cached
  }

  throw new Error(
    `Cannot load the @alienplatform/bindings native addon for '${triple}'. Install the '${pkg}' prebuild, or build it locally with \`napi build --platform\` in crates/alien-bindings-node, or set ALIEN_BINDINGS_ADDON_PATH to a built .node file.`,
  )
}

/** Test-only: reset the memoized addon so a fresh load can be observed. */
export function resetAddonCacheForTests(): void {
  cached = undefined
}
