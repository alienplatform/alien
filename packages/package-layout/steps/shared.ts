/**
 * Shared types and low-level helpers for the package-layout fixture steps.
 *
 * Each pipeline step lives in its own `steps/*.ts` file as a
 * `(ctx: Ctx) => CheckResult[]` function; they all depend on the `Ctx`/
 * `CheckResult` shapes and the process/text helpers here. `run.ts` threads the
 * shared context through the steps and reconciles their combined results.
 */

import { spawnSync } from "node:child_process"

/** One reported check. Failing ones are reconciled against expected-failures.json. */
export interface CheckResult {
  check: string
  package: string
  status: "pass" | "fail"
  reason: string
  evidence: string
  /**
   * Set only for checks run once per JS runtime (the `import`/`error-code`
   * family from src/imports.ts, executed under both Bun and Node). Used by
   * `detectRuntimeDivergence` to compare the same assertion across runtimes;
   * absent for checks that run exactly once regardless of runtime (pack,
   * install, typecheck, packed-contents, compile, validator).
   */
  runtime?: "bun" | "node"
}

/**
 * Shared state threaded through every step. Paths and `bunAvailable` are
 * computed once up front; `tarballs`, `addonPath`, and `addonEnv` are filled in
 * by earlier steps (pack, ensure-addon) and read by later ones (manifest,
 * import, compile).
 */
export interface Ctx {
  scriptDir: string
  packagesDir: string
  tarballsDir: string
  fixtureDir: string
  repoRoot: string
  validatorPath: string
  bunAvailable: boolean
  /** name -> absolute tarball path, for packages that packed successfully. */
  tarballs: Map<string, string>
  /** Resolved dev-addon path, when one had to be located or built for this host. */
  addonPath?: string
  /** Env carrying ALIEN_BINDINGS_ADDON_PATH for subprocesses, when `addonPath` is set. */
  addonEnv?: NodeJS.ProcessEnv
}

export interface RunOutput {
  status: number | null
  stdout: string
  stderr: string
}

export function run(
  command: string,
  args: string[],
  cwd: string,
  env?: NodeJS.ProcessEnv,
): RunOutput {
  const proc = spawnSync(command, args, {
    cwd,
    encoding: "utf8",
    env: env ? { ...process.env, ...env } : undefined,
  })
  if (proc.error) {
    return { status: null, stdout: proc.stdout ?? "", stderr: String(proc.error) }
  }
  return { status: proc.status, stdout: proc.stdout ?? "", stderr: proc.stderr ?? "" }
}

export function lastLine(text: string): string {
  const lines = text
    .split("\n")
    .map(line => line.trim())
    .filter(line => line.length > 0)
  return lines.at(-1) ?? ""
}

/** Escapes a string for literal use inside a RegExp. */
export function escapeRegExp(text: string): string {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")
}

// Hard denylist: never allowed in a packed tarball, regardless of `files` or
// EXTRA_SHIPPED_TODAY. Catches regressions even if an .npmignore is deleted.
// Shared by the packed-contents (sdk/core) and prebuild-packages steps.
export const HARD_DENYLIST_PATTERNS: RegExp[] = [
  /(^|\/)node_modules\//,
  /(^|\/)\.env$/,
  /\.tgz$/,
  /(^|\/)\.turbo\//,
]
