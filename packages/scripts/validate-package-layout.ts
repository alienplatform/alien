/**
 * Static validator enforcing the TypeScript package layout boundaries pinned in
 * packages/{sdk,bindings,commands}/PACKAGE_LAYOUT.md.
 *
 * Runs against the real tree. Any failure that is expected right now is
 * listed in expected-failures.json with its owning in-repo anchor; the run
 * exits 0 only when the actual failure set matches that list exactly (no
 * unexpected failures, no stale expectations).
 */

import { type Dirent, existsSync, readFileSync, readdirSync } from "node:fs"
import { basename, dirname, extname, join, relative, sep } from "node:path"
import { match } from "ts-pattern"

export interface Violation {
  check: string
  package: string
  reason: string
  evidence: string
}

export interface ExpectedFailureEntry {
  check: string
  package: string
  reason: string
  /** In-repo anchor (file or contract section) that owns fixing this failure. */
  owningTask: string
}

export interface FilterResult {
  expected: Violation[]
  fatal: Violation[]
  stale: ExpectedFailureEntry[]
}

/** Loose shape of a package.json — only the fields the checks below read. */
export interface PackageJsonLike {
  name?: string
  dependencies?: Record<string, string>
  devDependencies?: Record<string, string>
  peerDependencies?: Record<string, string>
  optionalDependencies?: Record<string, string>
  exports?: Record<string, unknown>
}

export type DependencyPackageKind = "bindings" | "commands"

const DEPENDENCY_SECTIONS = [
  "dependencies",
  "devDependencies",
  "peerDependencies",
  "optionalDependencies",
] as const

interface ForbiddenPattern {
  pattern: RegExp
  label: string
}

const FORBIDDEN_DEP_PATTERNS: Record<DependencyPackageKind, ForbiddenPattern[]> = {
  bindings: [
    { pattern: /^@aws-sdk\//, label: "forbidden dependency: AWS SDK" },
    { pattern: /^@google-cloud\//, label: "forbidden dependency: Google Cloud SDK" },
    { pattern: /^@azure\//, label: "forbidden dependency: Azure SDK" },
    { pattern: /^@grpc\/grpc-js$/, label: "forbidden dependency: gRPC (@grpc/grpc-js)" },
    { pattern: /^nice-grpc/, label: "forbidden dependency: nice-grpc" },
    { pattern: /^@alienplatform\/sdk$/, label: "forbidden dependency: @alienplatform/sdk" },
    {
      pattern: /^@alienplatform\/commands$/,
      label: "forbidden dependency: @alienplatform/commands",
    },
  ],
  commands: [
    { pattern: /^@grpc\/grpc-js$/, label: "forbidden dependency: gRPC (@grpc/grpc-js)" },
    { pattern: /^nice-grpc/, label: "forbidden dependency: nice-grpc" },
    {
      pattern: /^@alienplatform\/bindings$/,
      label: "forbidden dependency: @alienplatform/bindings",
    },
  ],
}

/**
 * Flags forbidden runtime/dev/peer/optional dependencies in a package.json for
 * a bindings-like or commands-like package (PACKAGE_LAYOUT.md "Dependency
 * boundaries" sections).
 */
export function checkForbiddenDeps(
  packageJsonPath: string,
  packageJson: PackageJsonLike,
  packageName: string,
  kind: DependencyPackageKind,
): Violation[] {
  const violations: Violation[] = []
  const rules = FORBIDDEN_DEP_PATTERNS[kind]

  for (const section of DEPENDENCY_SECTIONS) {
    const deps = packageJson[section]
    if (!deps) continue

    for (const depName of Object.keys(deps)) {
      for (const rule of rules) {
        if (rule.pattern.test(depName)) {
          violations.push({
            check: "forbidden-deps",
            package: packageName,
            reason: rule.label,
            evidence: `${packageJsonPath} (${section}.${depName})`,
          })
        }
      }
    }
  }

  return violations
}

// `nice-grpc` and `nice-grpc-common` are matched by two separate, explicit
// patterns (rather than one `nice-grpc` prefix match that happens to cover
// both) so that tightening either regex later cannot silently drop coverage
// of the other package. The `["'/]` terminator keeps `nice-grpc` from
// matching `nice-grpc-common` while still catching subpath imports.
const NICE_GRPC_IMPORT = /from\s+["']nice-grpc["'/]/
const NICE_GRPC_COMMON_IMPORT = /from\s+["']nice-grpc-common["'/]/
const GRPC_JS_IMPORT = /from\s+["']@grpc\/grpc-js/
// Generated Worker-protocol proto clients pulled in via a `generated/` path
// (e.g. `./generated/control.js`). Forbidden wholesale in bindings/commands —
// only the sdk's ./worker-runtime subpath may carry Worker protocol code.
const GENERATED_WORKER_PROTOCOL_IMPORT =
  /from\s+["'][^"']*generated\/(control|wait_until|worker)(\.js)?["']/

const FORBIDDEN_SOURCE_PATTERNS: Record<DependencyPackageKind, ForbiddenPattern[]> = {
  bindings: [
    { pattern: NICE_GRPC_IMPORT, label: "forbidden gRPC import (nice-grpc)" },
    { pattern: NICE_GRPC_COMMON_IMPORT, label: "forbidden gRPC import (nice-grpc-common)" },
    { pattern: GRPC_JS_IMPORT, label: "forbidden gRPC import (@grpc/grpc-js)" },
    { pattern: /from\s+["']@aws-sdk\//, label: "forbidden AWS SDK import" },
    { pattern: /from\s+["']@google-cloud\//, label: "forbidden Google Cloud SDK import" },
    { pattern: /from\s+["']@azure\//, label: "forbidden Azure SDK import" },
    {
      pattern: /from\s+["']@alienplatform\/sdk/,
      label: "forbidden import of @alienplatform/sdk (Worker protocol)",
    },
    {
      pattern: GENERATED_WORKER_PROTOCOL_IMPORT,
      label: "forbidden Worker protocol import (generated proto client)",
    },
    {
      pattern: /ALIEN_BINDINGS_GRPC_ADDRESS/,
      label: "forbidden env var reference: ALIEN_BINDINGS_GRPC_ADDRESS",
    },
    { pattern: /ALIEN_BINDINGS_MODE/, label: "forbidden env var reference: ALIEN_BINDINGS_MODE" },
  ],
  commands: [
    { pattern: NICE_GRPC_IMPORT, label: "forbidden gRPC import (nice-grpc)" },
    { pattern: NICE_GRPC_COMMON_IMPORT, label: "forbidden gRPC import (nice-grpc-common)" },
    { pattern: GRPC_JS_IMPORT, label: "forbidden gRPC import (@grpc/grpc-js)" },
    {
      pattern: /from\s+["']@alienplatform\/bindings/,
      label: "forbidden import of @alienplatform/bindings",
    },
    {
      // Any subpath of the sdk, including /worker-runtime — the commands
      // package is pure fetch and may not touch Worker app protocol at all.
      pattern: /from\s+["']@alienplatform\/sdk/,
      label: "forbidden import of @alienplatform/sdk (Worker protocol)",
    },
    {
      pattern: GENERATED_WORKER_PROTOCOL_IMPORT,
      label: "forbidden Worker protocol import (generated proto client)",
    },
  ],
}

const SKIPPED_DIR_NAMES = new Set(["node_modules", "dist", ".turbo"])

function isEnoent(error: unknown): boolean {
  return error instanceof Error && (error as NodeJS.ErrnoException).code === "ENOENT"
}

/**
 * Recursively collects `.ts`/`.tsx` source file paths under `dir`.
 *
 * A missing directory (ENOENT) yields no results — "no src/ yet" is a valid
 * state for a package this validator inspects. Every other filesystem error
 * (EACCES, ENOTDIR, I/O, …) propagates: silently treating those as "no
 * sources" would let the validator pass without having looked.
 *
 * Note: symlinks are not resolved — `Dirent.isDirectory()`/`isFile()` are
 * false for symlinks, so symlinked dirs/files are skipped, not followed.
 */
function walkSourceFiles(dir: string): string[] {
  const results: string[] = []
  let entries: Dirent<string>[]

  try {
    entries = readdirSync(dir, { withFileTypes: true })
  } catch (error) {
    if (isEnoent(error)) return results
    throw error
  }

  for (const entry of entries) {
    if (SKIPPED_DIR_NAMES.has(entry.name) || entry.name.startsWith(".")) continue

    const fullPath = join(dir, entry.name)
    if (entry.isDirectory()) {
      results.push(...walkSourceFiles(fullPath))
    } else if (entry.isFile() && (entry.name.endsWith(".ts") || entry.name.endsWith(".tsx"))) {
      results.push(fullPath)
    }
  }

  return results
}

/**
 * Flags forbidden imports and forbidden env-var references anywhere under a
 * bindings-like or commands-like package's source directory.
 */
export function checkForbiddenSources(
  dirPath: string,
  packageName: string,
  kind: DependencyPackageKind,
): Violation[] {
  const violations: Violation[] = []
  const rules = FORBIDDEN_SOURCE_PATTERNS[kind]

  for (const filePath of walkSourceFiles(dirPath)) {
    const lines = readFileSync(filePath, "utf8").split("\n")

    lines.forEach((line, index) => {
      for (const rule of rules) {
        if (rule.pattern.test(line)) {
          violations.push({
            check: "forbidden-sources",
            package: packageName,
            reason: rule.label,
            evidence: `${filePath}:${index + 1}`,
          })
        }
      }
    })
  }

  return violations
}

const WORKER_RUNTIME_DIRNAME = "worker-runtime"

const SDK_GRPC_IMPORT_PATTERNS: ForbiddenPattern[] = [
  {
    pattern: NICE_GRPC_IMPORT,
    label: "gRPC import (nice-grpc) outside ./worker-runtime source directory",
  },
  {
    pattern: NICE_GRPC_COMMON_IMPORT,
    label: "gRPC import (nice-grpc-common) outside ./worker-runtime source directory",
  },
  {
    pattern: GRPC_JS_IMPORT,
    label: "gRPC import (@grpc/grpc-js) outside ./worker-runtime source directory",
  },
]

/**
 * Generated ts-proto client basenames that back binding-service RPCs
 * (storage/kv/queue/vault + the deleted non-app kinds). Worker's own protocol
 * (control, wait_until) is intentionally excluded — that proto is permitted,
 * but only under ./worker-runtime.
 */
const BINDING_SERVICE_PROTO_BASENAMES = new Set([
  "artifact_registry",
  "build",
  "container",
  "kv",
  "queue",
  "service_account",
  "storage",
  "vault",
  "worker",
])

/**
 * Enforces the sdk package's `./worker-runtime` containment rule: gRPC /
 * Worker-protocol imports are forbidden outside `./worker-runtime`, and no
 * generated binding-service proto client may ship anywhere in the package.
 */
export function checkSdkSubpathContainment(srcDir: string, packageName = "sdk"): Violation[] {
  const violations: Violation[] = []

  for (const filePath of walkSourceFiles(srcDir)) {
    const relPath = relative(srcDir, filePath)
    const insideWorkerRuntime = relPath.split(sep)[0] === WORKER_RUNTIME_DIRNAME

    if (!insideWorkerRuntime) {
      const lines = readFileSync(filePath, "utf8").split("\n")
      lines.forEach((line, index) => {
        for (const rule of SDK_GRPC_IMPORT_PATTERNS) {
          if (rule.pattern.test(line)) {
            violations.push({
              check: "sdk-subpath-containment",
              package: packageName,
              reason: rule.label,
              evidence: `${filePath}:${index + 1}`,
            })
          }
        }
      })
    }

    const base = basename(filePath, extname(filePath))
    const parentDirName = basename(dirname(filePath))
    // Assumes generated proto clients sit as DIRECT children of a `generated/`
    // dir (the ts-proto layout in use today); clients nested one level deeper
    // (e.g. generated/foo/storage.ts) would not match this basename check.
    if (parentDirName === "generated" && BINDING_SERVICE_PROTO_BASENAMES.has(base)) {
      violations.push({
        check: "sdk-subpath-containment",
        package: packageName,
        reason: "generated binding-service proto client shipped in the package",
        evidence: filePath,
      })
    }
  }

  return violations
}

/** Flags a `package.json` `exports` map that still contains the deleted `./commands` subpath. */
export function checkNoCommandsSubpath(
  packageJsonPath: string,
  exportsMap: Record<string, unknown> | undefined,
  packageName: string,
): Violation[] {
  if (!exportsMap || !Object.hasOwn(exportsMap, "./commands")) return []

  return [
    {
      check: "no-commands-subpath",
      package: packageName,
      reason: "exports map still contains ./commands",
      evidence: `${packageJsonPath} exports["./commands"]`,
    },
  ]
}

/** Flags any `exports` subpath condition object missing a `types` entry. */
export function checkExportsTypes(
  packageJsonPath: string,
  exportsMap: Record<string, unknown> | undefined,
  packageName: string,
): Violation[] {
  if (!exportsMap) return []

  const violations: Violation[] = []

  for (const [subpath, condition] of Object.entries(exportsMap)) {
    const hasTypes =
      typeof condition === "object" && condition !== null && Object.hasOwn(condition, "types")

    if (!hasTypes) {
      violations.push({
        check: "exports-types",
        package: packageName,
        reason: `exports["${subpath}"] is missing a "types" condition`,
        evidence: `${packageJsonPath} exports["${subpath}"]`,
      })
    }
  }

  return violations
}

/** Stable identity for a violation/expected-failure: `check::package::reason`. */
export function expectedFailureKey(entry: {
  check: string
  package: string
  reason: string
}): string {
  return `${entry.check}::${entry.package}::${entry.reason}`
}

/**
 * Splits actual violations into `expected` (matches an entry in
 * expected-failures.json) and `fatal` (does not). Any expected-failures.json
 * entry that matched nothing real is returned as `stale` — a stale entry
 * fails the run too, so the list can't silently rot.
 */
export function applyExpectedFailures(
  violations: Violation[],
  expectedFailures: ExpectedFailureEntry[],
): FilterResult {
  const matchedKeys = new Set<string>()
  const expected: Violation[] = []
  const fatal: Violation[] = []

  for (const violation of violations) {
    const key = expectedFailureKey(violation)
    const isExpected = expectedFailures.some(entry => expectedFailureKey(entry) === key)

    if (isExpected) {
      matchedKeys.add(key)
      expected.push(violation)
    } else {
      fatal.push(violation)
    }
  }

  const stale = expectedFailures.filter(entry => !matchedKeys.has(expectedFailureKey(entry)))

  return { expected, fatal, stale }
}

/**
 * The run passes (exit 0) only when there are zero unexpected (fatal)
 * violations AND zero stale expectations. Expected violations never fail
 * the run; a stale expectation always does.
 */
export function exitCodeFor(result: FilterResult): 0 | 1 {
  return result.fatal.length === 0 && result.stale.length === 0 ? 0 : 1
}

// ---------------------------------------------------------------------------
// CLI entry point
// ---------------------------------------------------------------------------

function readPackageJson(packageJsonPath: string): PackageJsonLike {
  return JSON.parse(readFileSync(packageJsonPath, "utf8")) as PackageJsonLike
}

/**
 * Runs every check against the real packages/{sdk,bindings,commands} tree.
 * bindings/commands currently contain only PACKAGE_LAYOUT.md — that is
 * recorded as its own violation (`package-not-implemented`) rather than
 * silently skipped, so expected-failures.json has to name it explicitly.
 */
function collectViolations(packagesDir: string): Violation[] {
  const violations: Violation[] = []

  const sdkDir = join(packagesDir, "sdk")
  if (existsSync(join(sdkDir, "package.json"))) {
    const packageJsonPath = join(sdkDir, "package.json")
    const packageJson = readPackageJson(packageJsonPath)
    violations.push(...checkNoCommandsSubpath(packageJsonPath, packageJson.exports, "sdk"))
    violations.push(...checkExportsTypes(packageJsonPath, packageJson.exports, "sdk"))
    violations.push(...checkSdkSubpathContainment(join(sdkDir, "src"), "sdk"))
  }

  for (const [packageName, kind] of [
    ["bindings", "bindings"],
    ["commands", "commands"],
  ] as const) {
    const packageDir = join(packagesDir, packageName)
    const packageJsonPath = join(packageDir, "package.json")

    if (!existsSync(packageJsonPath)) {
      violations.push({
        check: "package-not-implemented",
        package: packageName,
        reason: "package not yet implemented (only PACKAGE_LAYOUT.md present)",
        evidence: packageDir,
      })
      continue
    }

    const packageJson = readPackageJson(packageJsonPath)
    violations.push(...checkForbiddenDeps(packageJsonPath, packageJson, packageName, kind))
    violations.push(...checkForbiddenSources(join(packageDir, "src"), packageName, kind))
    violations.push(...checkExportsTypes(packageJsonPath, packageJson.exports, packageName))
  }

  return violations
}

function loadExpectedFailures(expectedFailuresPath: string): ExpectedFailureEntry[] {
  return JSON.parse(readFileSync(expectedFailuresPath, "utf8")) as ExpectedFailureEntry[]
}

function printReport(result: FilterResult): void {
  for (const violation of result.expected) {
    console.log(`[expected] ${violation.check} package=${violation.package}: ${violation.reason}`)
    console.log(`           evidence: ${violation.evidence}`)
  }

  for (const violation of result.fatal) {
    console.error(`[FAIL] ${violation.check} package=${violation.package}: ${violation.reason}`)
    console.error(`       evidence: ${violation.evidence}`)
  }

  for (const staleEntry of result.stale) {
    console.error(
      `[STALE EXPECTATION] ${staleEntry.check} package=${staleEntry.package}: "${staleEntry.reason}" ` +
        `(owningTask ${staleEntry.owningTask}) is listed in expected-failures.json but no longer occurs — remove it or fix the check.`,
    )
  }

  const summary = match(result)
    .with(
      { fatal: [], stale: [] },
      () => `OK: ${result.expected.length} expected failure(s), 0 unexpected, 0 stale.`,
    )
    .otherwise(
      () =>
        `FAILED: ${result.fatal.length} unexpected failure(s), ${result.stale.length} stale expectation(s).`,
    )
  console.log(summary)
}

function main(): void {
  // packages/scripts -> packages
  const packagesDir = join(import.meta.dirname, "..")
  const expectedFailuresPath = join(import.meta.dirname, "expected-failures.json")

  if (!existsSync(packagesDir)) {
    console.error(`Cannot find packages directory at ${packagesDir}`)
    process.exit(1)
  }

  const violations = collectViolations(packagesDir)
  const expectedFailures = loadExpectedFailures(expectedFailuresPath)
  const result = applyExpectedFailures(violations, expectedFailures)

  printReport(result)

  process.exit(exitCodeFor(result))
}

/**
 * True when `moduleUrl` (the caller's `import.meta.url`) is the process entry
 * point rather than an imported dependency. Callers must pass their own
 * `import.meta.url` — it is module-scoped, so a shared helper cannot read it
 * for them.
 */
export function isMainModule(moduleUrl: string): boolean {
  return typeof process.argv[1] === "string" && moduleUrl === `file://${process.argv[1]}`
}

if (isMainModule(import.meta.url)) {
  main()
}
