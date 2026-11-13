/**
 * Reader for the bounded command credential an enrolled Operator mounts into a
 * workload (`ALIEN_COMMANDS_CAPABILITY_FILE`).
 *
 * Twin of the file source in the Rust `RuntimeCredentials`
 * (`alien-client-core`). The host rewrites the file before the credential
 * expires, so the file is read on every request and nothing is renewed here.
 * The file holds `{"token": "...", "expiresAt": "<RFC 3339>"}` and nothing
 * else. Anything that does not fit is refused, never sent to the manager.
 */

import { open } from "node:fs/promises"
import { AlienError } from "@alienplatform/core"
import { CommandReceiverConfigInvalidError } from "./errors.js"

export const ENV_ALIEN_COMMANDS_CAPABILITY_FILE = "ALIEN_COMMANDS_CAPABILITY_FILE"

const MAX_FILE_BYTES = 16 * 1024
const TOKEN_PREFIX = "ax_cap_v1_"
/** Longest lifetime the manager issues to a command receiver. */
const MAX_LIFETIME_MS = 60 * 60_000
/** Allowed clock difference between this process and the manager. */
const ALLOWED_CLOCK_SKEW_MS = 5 * 60_000
const RFC3339 = /^\d{4}-\d{2}-\d{2}[Tt]\d{2}:\d{2}:\d{2}(\.\d+)?([Zz]|[+-]\d{2}:\d{2})$/

export class CapabilityFile {
  private cached: string | undefined
  private rejected: string | undefined

  constructor(
    private readonly path: string,
    private readonly now: () => number = Date.now,
  ) {}

  /** The current capability. Throws a retryable error when the file is unusable. */
  async read(): Promise<string> {
    const text = await this.readText()
    const capability = this.parse(text)
    if (this.rejected === capability) {
      throw unusable("the capability file still contains a rejected credential")
    }
    this.cached = capability
    return capability
  }

  /** Record that the manager refused `token`, so the file must change before it is sent again. */
  reject(token: string): void {
    if (this.cached === token) {
      this.cached = undefined
      this.rejected = token
    }
  }

  private async readText(): Promise<string> {
    let handle: Awaited<ReturnType<typeof open>>
    try {
      handle = await open(this.path, "r")
    } catch (error) {
      throw unusable("cannot open the capability file", await AlienError.from(error))
    }
    try {
      const stat = await handle.stat()
      if (!stat.isFile() || stat.size > MAX_FILE_BYTES) {
        throw unusable("the capability file is not a regular file of at most 16 KiB")
      }
      // Projected Secrets are root-owned and readable by the pod's fsGroup. That
      // group shares the credential trust boundary. Anyone else must not read it.
      if (process.platform !== "win32") {
        const mode = stat.mode
        if ((mode & 0o037) !== 0 || ((mode & 0o040) !== 0 && stat.uid !== 0)) {
          throw unusable("the capability file must be private or root-owned group-read-only")
        }
      }
      const buffer = Buffer.alloc(MAX_FILE_BYTES + 1)
      const { bytesRead } = await handle.read(buffer, 0, buffer.length, 0)
      if (bytesRead > MAX_FILE_BYTES) {
        throw unusable("the capability file exceeds its size limit")
      }
      return buffer.subarray(0, bytesRead).toString("utf8")
    } catch (error) {
      if (error instanceof AlienError) throw error
      throw unusable("cannot read the capability file", await AlienError.from(error))
    } finally {
      await handle.close().catch(() => {})
    }
  }

  private parse(text: string): string {
    // The parser's own error text can quote the input, so it is not kept.
    let value: unknown
    try {
      value = JSON.parse(text)
    } catch {
      throw unusable("the capability file is not valid JSON")
    }
    if (
      typeof value !== "object" ||
      value === null ||
      Array.isArray(value) ||
      Object.keys(value).sort().join(",") !== "expiresAt,token"
    ) {
      throw unusable("the capability file must hold only token and expiresAt")
    }
    const { token, expiresAt } = value as { token: unknown; expiresAt: unknown }
    if (typeof token !== "string" || typeof expiresAt !== "string" || !RFC3339.test(expiresAt)) {
      throw unusable("the capability file has an invalid token or expiry")
    }
    const remainingMs = Date.parse(expiresAt) - this.now()
    if (
      !token.startsWith(TOKEN_PREFIX) ||
      !(remainingMs > 0) ||
      remainingMs > MAX_LIFETIME_MS + ALLOWED_CLOCK_SKEW_MS
    ) {
      throw unusable("the capability file has an invalid credential lifetime")
    }
    return token
  }
}

/**
 * The host writes the file and replaces it before expiry, so it can be
 * missing, late or stale for a while. The receiver keeps polling with backoff.
 */
function unusable(reason: string, source?: AlienError): AlienError {
  const context = { envVar: ENV_ALIEN_COMMANDS_CAPABILITY_FILE, reason }
  const definition = CommandReceiverConfigInvalidError.create(context)
  return new AlienError({
    code: definition.metadata.code,
    message: definition.metadata.message(context),
    retryable: true,
    internal: definition.metadata.internal,
    httpStatusCode: definition.metadata.httpStatusCode,
    context: definition.context,
    source,
  })
}
