/**
 * Locate (and, for a compiled Worker, extract) the `alien-ai-gateway` executable
 * that the gateway wrapper spawns.
 *
 * Resolution order (deferred to first spawn, so importing the package does no I/O):
 *
 *   1. An embedded binary registered by the `./native` entry. A `bun build
 *      --compile` Worker embeds the executable and hands us its (virtual) path;
 *      we copy it to a real, executable temp file once.
 *   2. `ALIEN_AI_GATEWAY_BINARY_PATH`: an explicit path (dev/test escape hatch;
 *      never set in published installs).
 *   3. The per-platform prebuild package `@alienplatform/ai-gateway-<triple>`,
 *      which ships the executable (installed via `optionalDependencies`).
 *   4. Dev fallback: the locally-built binary under `target/{release,debug}`,
 *      found by walking up from this module.
 */

import { chmodSync, existsSync, mkdtempSync, writeFileSync } from "node:fs"
import { createRequire } from "node:module"
import { tmpdir } from "node:os"
import { dirname, join, resolve } from "node:path"
import { fileURLToPath } from "node:url"
import { AlienError } from "@alienplatform/core"

import { GatewayBinaryUnavailableError, UnsupportedPlatformError } from "./errors.js"

const require = createRequire(import.meta.url)

/** The launcher binary's file name across all platforms. */
const BINARY_NAME = "alien-ai-gateway"

/** A running gateway handle: its loopback base URL. */
export interface RawAiGatewayHandle {
  readonly url: string
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

/**
 * Map `process.platform`/`arch` to the prebuild triple. The binary is built for
 * musl as well as glibc, so Alpine/musl images resolve a prebuild rather than
 * being rejected for their libc.
 */
export function platformTriple(
  platform: NodeJS.Platform = process.platform,
  arch: NodeJS.Architecture = process.arch,
  libc: LinuxLibc = platform === "linux" ? detectLinuxLibc() : "gnu",
): string {
  if (platform === "darwin" && arch === "arm64") return "darwin-arm64"
  if (platform === "darwin" && arch === "x64") return "darwin-x64"
  if (platform === "linux") {
    if (arch === "x64") return `linux-x64-${libc}`
    if (arch === "arm64") return `linux-arm64-${libc}`
  }
  throw new AlienError(UnsupportedPlatformError.create({ platform, arch }))
}

/** Walk up from `startDir` to find the locally-built binary, or `undefined`. */
export function findLocalBinary(
  startDir: string = dirname(fileURLToPath(import.meta.url)),
): string | undefined {
  let dir = startDir
  for (;;) {
    for (const profile of ["release", "debug"]) {
      const candidate = join(dir, "target", profile, BINARY_NAME)
      if (existsSync(candidate)) return candidate
    }
    const parent = dirname(dir)
    if (parent === dir) return undefined
    dir = parent
  }
}

let cached: string | undefined
let embeddedBinaryPath: string | undefined

/**
 * Register the bun-embedded binary path, so plain `@alienplatform/ai-gateway`
 * imports (which resolve through {@link resolveGatewayBinary}) find it inside a
 * `bun build --compile` binary, where the filesystem/prebuild resolution below
 * cannot. In a normal install this is never called. The `/native` entry calls it.
 */
export function registerEmbeddedBinary(path: string): void {
  embeddedBinaryPath = path
}

/** Resolve (and memoize) a runnable path to the launcher binary, or throw. */
export async function resolveGatewayBinary(): Promise<string> {
  if (cached) return cached

  // A compiled binary registers its embedded copy up front; extract it to a real,
  // executable file, since the embedded path is virtual and cannot be spawned.
  if (embeddedBinaryPath) {
    cached = await extractEmbedded(embeddedBinaryPath)
    return cached
  }

  const override = process.env.ALIEN_AI_GATEWAY_BINARY_PATH
  if (override) {
    // Resolved against cwd, not this module's dist/ directory.
    cached = ensureExecutable(resolve(override))
    return cached
  }

  const triple = platformTriple()
  const prebuild = await resolvePrebuild(triple)
  if (prebuild) {
    cached = ensureExecutable(prebuild)
    return cached
  }

  const local = findLocalBinary()
  if (local) {
    cached = ensureExecutable(local)
    return cached
  }

  throw new AlienError(
    GatewayBinaryUnavailableError.create({
      triple,
      reason: `no embedded binary, no ALIEN_AI_GATEWAY_BINARY_PATH, no '@alienplatform/ai-gateway-${triple}' prebuild, and no locally-built target/{release,debug}/${BINARY_NAME}; build it with \`cargo build --bin ${BINARY_NAME} -p alien-ai-gateway\``,
    }),
  )
}

/**
 * Copy a bun-embedded binary to a real, executable temp file. Only reached inside
 * a `bun build --compile` binary, where `Bun` is present and the embedded file
 * must live on disk with the execute bit set before it can be spawned.
 */
async function extractEmbedded(virtualPath: string): Promise<string> {
  const bun = (globalThis as { Bun?: { file(p: string): { arrayBuffer(): Promise<ArrayBuffer> } } })
    .Bun
  if (!bun) {
    throw new AlienError(
      GatewayBinaryUnavailableError.create({
        triple: platformTriple(),
        path: virtualPath,
        reason: "an embedded gateway binary can only be extracted under the Bun runtime",
      }),
    )
  }
  try {
    const bytes = await bun.file(virtualPath).arrayBuffer()
    const dir = mkdtempSync(join(tmpdir(), "alien-ai-gateway-"))
    const exe = join(dir, BINARY_NAME)
    writeFileSync(exe, Buffer.from(bytes))
    chmodSync(exe, 0o755)
    return exe
  } catch (error) {
    // A filesystem fault here (no temp space, read-only tmp) would otherwise
    // surface as a bare Error; wrap it so callers still see an AlienError.
    throw (await AlienError.from(error)).withContext(
      GatewayBinaryUnavailableError.create({
        triple: platformTriple(),
        path: virtualPath,
        reason: "failed to extract the embedded gateway binary",
      }),
    )
  }
}

/** Best-effort execute bit; a prebuild may already be executable or read-only. */
function ensureExecutable(path: string): string {
  try {
    chmodSync(path, 0o755)
  } catch {
    // A read-only mount can't be chmod'd; if it isn't already executable the
    // spawn below surfaces a real, specific error.
  }
  return path
}

/** Resolve the binary shipped by the per-platform prebuild package, or `undefined`. */
async function resolvePrebuild(triple: string): Promise<string | undefined> {
  const pkg = `@alienplatform/ai-gateway-${triple}`
  let pkgJson: string
  try {
    pkgJson = require.resolve(`${pkg}/package.json`)
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "MODULE_NOT_FOUND") return undefined
    // Installed but unresolvable (e.g. a corrupt manifest); preserve the cause.
    throw (await AlienError.from(error)).withContext(
      GatewayBinaryUnavailableError.create({
        triple,
        reason: `the '${pkg}' prebuild is installed but could not be resolved`,
      }),
    )
  }
  const candidate = join(dirname(pkgJson), BINARY_NAME)
  return existsSync(candidate) ? candidate : undefined
}

/** Test-only: reset the memoized resolution. */
export function resetGatewayBinaryCacheForTests(): void {
  cached = undefined
  embeddedBinaryPath = undefined
}
