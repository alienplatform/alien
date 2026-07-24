/**
 * The gateway surface: obtain the loopback URL of the Alien AI gateway.
 *
 * Two paths, unified behind {@link Gateway.startAiGateway}:
 *   - A container launcher already started the gateway out of band and exec'd the
 *     app with `ALIEN_AI_GATEWAY_URL` set, so we use that URL directly.
 *   - Otherwise (an SDK Worker) we spawn the `alien-ai-gateway` binary ourselves,
 *     read the URL it prints, and keep the child alive for the process lifetime.
 *
 * Parameterized over how the binary is resolved so the lazy entry (`index.ts`) and
 * the compiled-embed entry (`native.ts`) share one implementation.
 */

import { type ChildProcess, spawn } from "node:child_process"
import { AlienError } from "@alienplatform/core"

import { GatewayBinaryUnavailableError, GatewayStartFailedError } from "./errors.js"
import { type RawAiGatewayHandle, platformTriple } from "./loader.js"

/** The env var a container launcher sets after starting the gateway out of band. */
const URL_ENV = "ALIEN_AI_GATEWAY_URL"
/** The launcher serve mode; binds an ephemeral port and prints its URL. */
const SERVE_FLAG = "--gateway-serve"

export interface Gateway {
  startAiGateway(): Promise<RawAiGatewayHandle>
}

export function createGateway(resolveBinary: () => Promise<string>): Gateway {
  // Started once per process: the child is held for the process lifetime, since
  // killing it stops the gateway. Only a *resolved* start is memoized. Caching a
  // rejection would turn one transient startup failure into a permanently dead
  // gateway, even though the Rust side marks those errors retryable; and a child
  // that dies after reporting ready clears the memo (see keepChildAlive), so the
  // next call respawns instead of handing back a URL nothing is listening on.
  let started: Promise<RawAiGatewayHandle> | null = null
  const forget = () => {
    started = null
  }

  async function startAiGateway(): Promise<RawAiGatewayHandle> {
    started ??= startOnce(resolveBinary, forget).catch(error => {
      forget()
      throw error
    })
    return started
  }

  return { startAiGateway }
}

async function startOnce(
  resolveBinary: () => Promise<string>,
  onGatewayLost: () => void,
): Promise<RawAiGatewayHandle> {
  // A launcher (container path) already started the gateway and exported its URL;
  // it owns that process, so there is nothing here to keep alive or reap.
  const preset = process.env[URL_ENV]
  if (preset) return { url: preset }

  const binary = await resolveBinary()
  const child = spawn(binary, [SERVE_FLAG], { stdio: ["ignore", "pipe", "pipe"] })
  // Track and guard the child the instant it exists, before the ready-await: a
  // directed SIGTERM (or a stream I/O fault) during startup must reap it, not
  // orphan it.
  trackChild(child)
  const url = await readReadyUrl(child, binary)
  keepChildAlive(child, onGatewayLost)
  return { url }
}

/**
 * Read the gateway's `{"aiGatewayUrl":"..."}` line from stdout. Rejects (with the
 * child's stderr as the reason) if the process exits or errors before printing it.
 */
function readReadyUrl(child: ChildProcess, binary: string): Promise<string> {
  return new Promise<string>((resolveUrl, rejectUrl) => {
    let stdout = ""
    let stderr = ""

    const cleanup = () => {
      child.stdout?.off("data", onStdout)
      child.stderr?.off("data", onStderr)
      child.off("exit", onExit)
      child.off("error", onError)
    }

    const onStdout = (chunk: Buffer) => {
      stdout += chunk.toString()
      const url = parseReadyUrl(stdout)
      if (url) {
        cleanup()
        resolveUrl(url)
      }
    }
    const onStderr = (chunk: Buffer) => {
      stderr += chunk.toString()
    }
    // An early exit is usually transient (an ambient cloud credential not yet
    // resolvable), so surface it as retryable.
    const onExit = (code: number | null, signal: NodeJS.Signals | null) => {
      cleanup()
      rejectUrl(
        new AlienError(
          GatewayStartFailedError.create({
            reason:
              stderr.trim() ||
              `process exited (${signal ?? `code ${code ?? "null"}`}) before reporting a URL`,
          }),
        ),
      )
    }
    // A spawn 'error' means the OS could not exec the binary (ENOENT / EACCES /
    // exec-format): the host cannot run it, so a retry against the same path is
    // futile. Surface it as the non-retryable "binary unavailable" class.
    const onError = async (error: Error) => {
      cleanup()
      // platformTriple() can throw on an unsupported host, and this runs inside an
      // event callback where a throw would strand the promise; fall back to the raw
      // platform/arch so the rejection always fires.
      let triple: string
      try {
        triple = platformTriple()
      } catch {
        triple = `${process.platform}-${process.arch}`
      }
      // Chain the spawn error (ENOENT/EACCES) so its cause is preserved.
      rejectUrl(
        (await AlienError.from(error)).withContext(
          GatewayBinaryUnavailableError.create({
            triple,
            path: binary,
            reason: "could not execute the gateway binary",
          }),
        ),
      )
    }

    child.stdout?.on("data", onStdout)
    child.stderr?.on("data", onStderr)
    child.on("exit", onExit)
    child.on("error", onError)
  })
}

/** Extract the URL from the launcher's machine-readable stdout line, if present. */
function parseReadyUrl(buffered: string): string | undefined {
  for (const line of buffered.split("\n")) {
    const trimmed = line.trim()
    if (!trimmed.startsWith("{")) continue
    try {
      const parsed = JSON.parse(trimmed) as { aiGatewayUrl?: unknown }
      if (typeof parsed.aiGatewayUrl === "string") return parsed.aiGatewayUrl
    } catch {
      // A partial line: wait for more stdout.
    }
  }
  return undefined
}

// The child currently serving the gateway. The reaper (installed once) reads this,
// so a respawn never stacks duplicate signal handlers and never leaves a previous
// child orphaned when the process is torn down.
let liveChild: ChildProcess | undefined
let reaperInstalled = false

/** Reap whatever child is live when this process is torn down. Installed once. */
function installReaper(): void {
  if (reaperInstalled) return
  reaperInstalled = true
  const killLive = () => {
    try {
      liveChild?.kill("SIGTERM")
    } catch {
      // Already gone.
    }
  }
  // Normal exit reaps synchronously.
  process.once("exit", killLive)
  // A directed SIGINT/SIGTERM terminates the host before 'exit' fires, which would
  // orphan the child. Reap it, then re-raise the signal so the host app's own
  // handlers (or the default action) still run: we don't force-exit and truncate
  // its shutdown.
  for (const signal of ["SIGINT", "SIGTERM"] as const) {
    process.once(signal, () => {
      killLive()
      process.kill(process.pid, signal)
    })
  }
}

/**
 * Track a freshly spawned child as the live gateway and guard it synchronously, at
 * spawn time (before the ready-await). A live child stream with no 'error' listener
 * throws on any I/O fault and crashes the host, so attach permanent no-op stream
 * guards now (`readReadyUrl` only adds 'data' listeners; the child itself always has
 * an 'error' listener, `readReadyUrl`'s during startup then `keepChildAlive`'s after).
 */
function trackChild(child: ChildProcess): void {
  liveChild = child
  installReaper()
  child.stdout?.on("error", () => {})
  child.stderr?.on("error", () => {})
}

/**
 * Once the child is serving, hold it for the app's lifetime: drain its stdio so the
 * pipes never fill and block it, and `unref` so it never keeps the event loop alive
 * on its own. On a later exit/error, forget the memoized handle so the next call
 * respawns rather than reusing a dead URL (`trackChild` already installed the reaper
 * and stream guards at spawn time).
 */
function keepChildAlive(child: ChildProcess, onGatewayLost: () => void): void {
  const onGone = () => {
    if (liveChild === child) liveChild = undefined
    onGatewayLost()
  }
  child.on("error", onGone)
  child.on("exit", onGone)
  child.stdout?.resume()
  child.stderr?.resume()
  child.unref()
}
