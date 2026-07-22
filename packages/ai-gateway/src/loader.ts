/**
 * Native addon loader for the AI gateway.
 *
 * Resolves the napi-rs addon for the current platform, in order (mirrors
 * `@alienplatform/bindings`):
 *
 *   1. `ALIEN_AI_GATEWAY_ADDON_PATH` — an explicit path to a `.node` file
 *      (dev/test escape hatch; never set in published installs).
 *   2. The per-platform prebuild package `@alienplatform/ai-gateway-<triple>`
 *      from `optionalDependencies` (injected at publish time; absent in dev).
 *   3. Dev fallback: the locally-built addon under
 *      `crates/alien-ai-gateway-node/alien-ai-gateway-node.<triple>.node`, found
 *      by walking up from this module, version-gated against this package.
 *
 * Loading is deferred to first use, so importing the package performs no I/O.
 */

import { existsSync } from "node:fs"
import { createRequire } from "node:module"
import { dirname, join, resolve } from "node:path"
import { fileURLToPath } from "node:url"
import { AlienError } from "@alienplatform/core"

import { NativeAddonLoadFailedError, UnsupportedPlatformError } from "./errors.js"

const require = createRequire(import.meta.url)

/** A running gateway handle from the addon: its loopback base URL. Held for the
 * process lifetime — dropping the last reference stops the server. */
export interface RawAiGatewayHandle {
  readonly url: string
}

/** The complete napi addon surface consumed by the wrapper. */
export interface NativeAddon {
  startAiGateway(): Promise<RawAiGatewayHandle>
  version(): string
}

export type LinuxLibc = "gnu" | "musl"

/** Detect glibc vs musl, the same way napi-rs's generated loader does. */
export function detectLinuxLibc(): LinuxLibc {
  const report =
    typeof process.report?.getReport === "function"
      ? (process.report.getReport() as { header?: { glibcVersionRuntime?: string } })
      : undefined
  if (report?.header) {
    return report.header.glibcVersionRuntime ? "gnu" : "musl"
  }
  return existsSync("/etc/alpine-release") ? "musl" : "gnu"
}

/** Map `process.platform`/`arch` to the napi triple (prebuild name + `.node` file name). */
export function platformTriple(
  platform: NodeJS.Platform = process.platform,
  arch: NodeJS.Architecture = process.arch,
  libc: LinuxLibc = platform === "linux" ? detectLinuxLibc() : "gnu",
): string {
  if (platform === "darwin" && arch === "arm64") return "darwin-arm64"
  if (platform === "darwin" && arch === "x64") return "darwin-x64"
  if (platform === "linux" && libc === "musl") {
    throw new AlienError(
      UnsupportedPlatformError.create({
        platform,
        arch,
        reason:
          "prebuilds are published for glibc Linux only; run on a glibc-based image (debian/ubuntu-slim)",
      }),
    )
  }
  if (platform === "linux" && arch === "x64") return "linux-x64-gnu"
  if (platform === "linux" && arch === "arm64") return "linux-arm64-gnu"
  throw new AlienError(UnsupportedPlatformError.create({ platform, arch }))
}

/** Walk up from `startDir` to find the locally-built addon, or `undefined`. */
export function findLocalAddon(
  triple: string,
  startDir: string = dirname(fileURLToPath(import.meta.url)),
): string | undefined {
  const fileName = `alien-ai-gateway-node.${triple}.node`
  let dir = startDir
  for (;;) {
    const candidate = join(dir, "crates", "alien-ai-gateway-node", fileName)
    if (existsSync(candidate)) return candidate
    const parent = dirname(dir)
    if (parent === dir) return undefined
    dir = parent
  }
}

function packageVersion(): string {
  const dir = dirname(fileURLToPath(import.meta.url))
  const packageJson = require(join(dir, "..", "package.json")) as { version: string }
  return packageJson.version
}

let cached: NativeAddon | undefined
let embedded: NativeAddon | undefined

/**
 * Register a bun-embedded addon with the default loader, so plain
 * `@alienplatform/ai-gateway` imports (which go through {@link loadAddon})
 * resolve to it inside a `bun build --compile` binary — where the
 * filesystem/prebuild resolution below cannot find the addon. In a normal
 * install this is never called and {@link loadAddon} falls through to its
 * normal resolution. The `/native` entry calls this at bootstrap.
 */
export function registerEmbeddedAddon(addon: NativeAddon): void {
  embedded = addon
}

/** Load (and memoize) the native addon, or throw if none resolves for this platform. */
export function loadAddon(): NativeAddon {
  if (cached) return cached

  // A compiled binary registers its embedded addon up front; prefer it over the
  // filesystem/prebuild resolution below, which cannot work inside the binary.
  if (embedded) {
    cached = embedded
    return cached
  }

  const triple = platformTriple()
  const pkg = `@alienplatform/ai-gateway-${triple}`

  const override = process.env.ALIEN_AI_GATEWAY_ADDON_PATH
  if (override) {
    // Resolved against cwd, not this module's dist/ directory.
    cached = requireAddon(resolve(override), triple, "ALIEN_AI_GATEWAY_ADDON_PATH")
    return cached
  }

  const prebuild = requirePrebuild(pkg, triple)
  if (prebuild) {
    cached = prebuild
    return cached
  }

  const local = findLocalAddon(triple)
  if (local) {
    const addon = requireAddon(local, triple, "the locally-built addon")
    const expected = packageVersion()
    const actual = addon.version()
    // Trust the local addon only when its reported version matches the installed package
    // version. A mismatch is a stale build from an earlier checkout (ABI/version skew).
    if (actual === expected) {
      cached = addon
      return cached
    }
    throw new AlienError(
      NativeAddonLoadFailedError.create({
        triple,
        path: local,
        reason: `the locally-built addon reports version '${actual}', but this package is '${expected}' — rebuild it with \`napi build --platform\` in crates/alien-ai-gateway-node`,
      }),
    )
  }

  throw new AlienError(
    NativeAddonLoadFailedError.create({
      triple,
      reason: `no addon found — install the '${pkg}' prebuild, build it locally with \`napi build --platform\` in crates/alien-ai-gateway-node, or set ALIEN_AI_GATEWAY_ADDON_PATH to a built .node file`,
    }),
  )
}

/** `require` the prebuild, or `undefined` when it is not installed. */
// Preserve the caught `require()` error as a structured source. This path is synchronous
// (CJS require) and so cannot use the async `AlienError.from`; building the source inline keeps
// the original dlopen/ABI failure (stack included) from being lost behind the generic message.
function nativeAddonLoadFailure(
  triple: string,
  path: string,
  reason: string,
  cause: unknown,
): AlienError {
  return new AlienError({
    ...NativeAddonLoadFailedError.create({ triple, path, reason }).toOptions(),
    source: {
      code: "GENERIC_ERROR",
      message: cause instanceof Error ? (cause.stack ?? cause.message) : String(cause),
      retryable: false,
      internal: true,
    },
  })
}

function requirePrebuild(pkg: string, triple: string): NativeAddon | undefined {
  try {
    return require(pkg) as NativeAddon
  } catch (error) {
    // Only "not installed" falls through to the dev-built addon. Anything else — a failed
    // dlopen from ABI skew, a corrupt .node, a broken transitive require — is a real
    // failure that the generic "install the prebuild" advice would misdiagnose.
    if ((error as NodeJS.ErrnoException).code === "MODULE_NOT_FOUND") {
      return undefined
    }
    throw nativeAddonLoadFailure(
      triple,
      pkg,
      error instanceof Error ? error.message : String(error),
      error,
    )
  }
}

function requireAddon(path: string, triple: string, source: string): NativeAddon {
  try {
    return require(path) as NativeAddon
  } catch (error) {
    throw nativeAddonLoadFailure(
      triple,
      path,
      `${source} could not be loaded: ${error instanceof Error ? error.message : String(error)}`,
      error,
    )
  }
}

/** Test-only: reset the memoized addon. */
export function resetAddonCacheForTests(): void {
  cached = undefined
  embedded = undefined
}
