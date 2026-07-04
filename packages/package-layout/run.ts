/**
 * Package-layout fixture orchestrator.
 *
 * Runs the executable proof that the packed publishable packages install and
 * import correctly on Bun and Node, honoring the pinned surfaces in
 * packages/{sdk,bindings,commands}/PACKAGE_LAYOUT.md. Discrete steps:
 *
 *   1. pnpm pack each existing publishable package (sdk, core; bindings/commands
 *      when they land) into .tarballs/.
 *   2. Rewrite the consumer's `dependencies` + `overrides` to file: those tarballs
 *      (every transitive @alienplatform/* pinned to a tarball, never npm).
 *   3. npm install the consumer (npm, not pnpm — a real consumer) and assert the
 *      @alienplatform/* packages resolved to the tarballs.
 *   4/5. Import check under Bun and Node (src/imports.ts) — pinned surfaces +
 *      BINDING_NOT_CONFIGURED / COMMAND_RECEIVER_CONFIG_INVALID by `code`.
 *   6. tsc typecheck of the consumer.
 *   7. packed-contents check (tar -tzf) of each tarball.
 *   8. bun build --compile of src/compile-entry.ts (the ./native embed) + run it.
 *   9. Invoke the static validator (packages/scripts/validate-package-layout.ts).
 *
 * Every step is individually reported (name, PASS/[expected]/FAIL, evidence).
 * Failing checks are reconciled against expected-failures.json exactly like the
 * validator does — the run exits 0 only when there are zero unexpected failures
 * and zero stale expectations (reused applyExpectedFailures + exitCodeFor).
 */

import { spawnSync } from "node:child_process"
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs"
import { dirname, join, relative } from "node:path"
import { fileURLToPath } from "node:url"
import {
  type ExpectedFailureEntry,
  type Violation,
  applyExpectedFailures,
  exitCodeFor,
} from "../scripts/validate-package-layout.ts"

const scriptDir = dirname(fileURLToPath(import.meta.url))
const packagesDir = join(scriptDir, "..")
const tarballsDir = join(scriptDir, ".tarballs")
const fixtureDir = join(scriptDir, "fixture")
const validatorPath = join(packagesDir, "scripts", "validate-package-layout.ts")

/** One reported check. Failing ones are reconciled against expected-failures.json. */
interface CheckResult {
  check: string
  package: string
  status: "pass" | "fail"
  reason: string
  evidence: string
}

const results: CheckResult[] = []
function record(result: CheckResult): void {
  results.push(result)
}

interface RunOutput {
  status: number | null
  stdout: string
  stderr: string
}

function run(command: string, args: string[], cwd: string): RunOutput {
  const proc = spawnSync(command, args, { cwd, encoding: "utf8" })
  if (proc.error) {
    return { status: null, stdout: proc.stdout ?? "", stderr: String(proc.error) }
  }
  return { status: proc.status, stdout: proc.stdout ?? "", stderr: proc.stderr ?? "" }
}

function lastLine(text: string): string {
  const lines = text
    .split("\n")
    .map(line => line.trim())
    .filter(line => line.length > 0)
  return lines.at(-1) ?? ""
}

// ---------------------------------------------------------------------------
// Environment
// ---------------------------------------------------------------------------

const bunAvailable = run("bun", ["--version"], scriptDir).status === 0
if (!bunAvailable) {
  console.log(
    "[env-skip] bun is not on PATH — the Bun import and `bun build --compile` steps are " +
      "skipped in this environment. CI provides bun via setup-bun.",
  )
}

// ---------------------------------------------------------------------------
// Step 1 — pack the publishable packages
// ---------------------------------------------------------------------------

rmSync(tarballsDir, { recursive: true, force: true })
mkdirSync(tarballsDir, { recursive: true })

/** name -> absolute tarball path, for packages that packed successfully. */
const tarballs = new Map<string, string>()

const PACK_TARGETS: { name: string; owningReason: string }[] = [
  { name: "sdk", owningReason: "" },
  { name: "core", owningReason: "" },
  { name: "bindings", owningReason: "publishable package not present (nothing to pack)" },
  { name: "commands", owningReason: "publishable package not present (nothing to pack)" },
]

function findTarball(name: string): string | undefined {
  const prefix = `alienplatform-${name}-`
  const file = readdirSync(tarballsDir).find(
    entry => entry.startsWith(prefix) && entry.endsWith(".tgz"),
  )
  return file ? join(tarballsDir, file) : undefined
}

for (const target of PACK_TARGETS) {
  const pkgDir = join(packagesDir, target.name)
  if (!existsSync(join(pkgDir, "package.json"))) {
    record({
      check: "pack",
      package: target.name,
      status: "fail",
      reason: target.owningReason || "publishable package not present (nothing to pack)",
      evidence: pkgDir,
    })
    continue
  }

  const packed = run("pnpm", ["pack", "--pack-destination", tarballsDir], pkgDir)
  const tarball = findTarball(target.name)
  if (packed.status !== 0 || !tarball) {
    record({
      check: "pack",
      package: target.name,
      status: "fail",
      reason: "pnpm pack failed",
      evidence: lastLine(packed.stderr) || lastLine(packed.stdout) || `exit ${packed.status}`,
    })
    continue
  }
  tarballs.set(target.name, tarball)
  record({
    check: "pack",
    package: target.name,
    status: "pass",
    reason: "ok",
    evidence: relative(scriptDir, tarball),
  })
}

// ---------------------------------------------------------------------------
// Step 2 — rewrite the consumer manifest to file: the tarballs
// ---------------------------------------------------------------------------

// Direct dependencies the consumer imports; overrides pin every transitive
// @alienplatform/* to a packed tarball so npm never reaches the registry for one.
const DIRECT_DEP_PACKAGES = ["sdk", "bindings", "commands"] as const
const OVERRIDE_PACKAGES = ["core", "sdk", "bindings", "commands"] as const

function fileSpec(tarball: string): string {
  return `file:${relative(fixtureDir, tarball).split("\\").join("/")}`
}

const fixtureManifestPath = join(fixtureDir, "package.json")
const fixtureManifest = JSON.parse(readFileSync(fixtureManifestPath, "utf8")) as Record<
  string,
  unknown
>

const dependencies: Record<string, string> = {}
for (const name of DIRECT_DEP_PACKAGES) {
  const tarball = tarballs.get(name)
  if (tarball) dependencies[`@alienplatform/${name}`] = fileSpec(tarball)
}
const overrides: Record<string, string> = {}
for (const name of OVERRIDE_PACKAGES) {
  const tarball = tarballs.get(name)
  if (tarball) overrides[`@alienplatform/${name}`] = fileSpec(tarball)
}

fixtureManifest.dependencies = dependencies
fixtureManifest.overrides = overrides
writeFileSync(fixtureManifestPath, `${JSON.stringify(fixtureManifest, null, 2)}\n`)
record({
  check: "write-manifest",
  package: "fixture",
  status: "pass",
  reason: "ok",
  evidence: `deps=[${Object.keys(dependencies).join(", ")}] overrides=[${Object.keys(overrides).join(", ")}]`,
})

// ---------------------------------------------------------------------------
// Step 3 — npm install the consumer, assert tarball resolution
// ---------------------------------------------------------------------------

// Force a clean resolution against the freshly rewritten manifest.
rmSync(join(fixtureDir, "node_modules"), { recursive: true, force: true })
rmSync(join(fixtureDir, "package-lock.json"), { force: true })

const install = run("npm", ["install", "--no-audit", "--no-fund"], fixtureDir)
if (install.status !== 0) {
  record({
    check: "install",
    package: "fixture",
    status: "fail",
    reason: "npm install failed",
    evidence: lastLine(install.stderr) || lastLine(install.stdout) || `exit ${install.status}`,
  })
} else {
  record({
    check: "install",
    package: "fixture",
    status: "pass",
    reason: "ok",
    evidence: lastLine(install.stdout),
  })

  const lockPath = join(fixtureDir, "package-lock.json")
  const lock = JSON.parse(readFileSync(lockPath, "utf8")) as {
    packages?: Record<string, { resolved?: string }>
  }
  const lockPackages = lock.packages ?? {}
  for (const name of ["sdk", "core"]) {
    if (!tarballs.has(name)) continue
    const entry = lockPackages[`node_modules/@alienplatform/${name}`]
    const resolved = entry?.resolved ?? "<not installed>"
    record({
      check: "install-resolution",
      package: name,
      status: resolved.startsWith("file:") ? "pass" : "fail",
      reason: resolved.startsWith("file:")
        ? "ok"
        : "transitive @alienplatform package did not resolve to the packed tarball",
      evidence: `node_modules/@alienplatform/${name} -> ${resolved}`,
    })
  }
}

// ---------------------------------------------------------------------------
// Steps 4/5 — import check under Bun and Node
// ---------------------------------------------------------------------------

const IMPORTS_ENTRY = join("src", "imports.ts")

function runImportCheck(runtime: "bun" | "node"): void {
  const output =
    runtime === "bun"
      ? run("bun", [IMPORTS_ENTRY], fixtureDir)
      : run("node", ["--experimental-strip-types", IMPORTS_ENTRY], fixtureDir)

  const lines = output.stdout.split("\n").filter(line => line.startsWith("##CHECK## "))
  if (lines.length === 0) {
    record({
      check: `${runtime}-imports`,
      package: "fixture",
      status: "fail",
      reason: "import check produced no results (crashed before reporting)",
      evidence: lastLine(output.stderr) || lastLine(output.stdout) || `exit ${output.status}`,
    })
    return
  }

  for (const line of lines) {
    const parsed = JSON.parse(line.slice("##CHECK## ".length)) as CheckResult
    record({
      check: parsed.check,
      package: parsed.package,
      status: parsed.status,
      reason: parsed.reason,
      evidence: `[${runtime}] ${parsed.evidence}`,
    })
  }
}

if (bunAvailable) runImportCheck("bun")
runImportCheck("node")

// ---------------------------------------------------------------------------
// Step 6 — tsc typecheck of the consumer
// ---------------------------------------------------------------------------

// Modules that are legitimately unresolvable today, with the task that lands them.
const EXPECTED_MISSING_MODULES: Record<string, { package: string }> = {
  "@alienplatform/bindings": { package: "bindings" },
  "@alienplatform/bindings/native": { package: "bindings" },
  "@alienplatform/commands": { package: "commands" },
  "@alienplatform/sdk/worker-runtime": { package: "sdk" },
}

const tscBin = join(fixtureDir, "node_modules", "typescript", "bin", "tsc")
if (!existsSync(tscBin)) {
  record({
    check: "typecheck",
    package: "fixture",
    status: "fail",
    reason: "typescript is not installed in the consumer (install step failed?)",
    evidence: tscBin,
  })
} else {
  const tsc = run("node", [tscBin, "--noEmit", "-p", "tsconfig.json"], fixtureDir)
  if (tsc.status === 0) {
    record({
      check: "typecheck",
      package: "fixture",
      status: "pass",
      reason: "ok",
      evidence: "tsc --noEmit reported no errors",
    })
  } else {
    const errorLines = tsc.stdout.split("\n").filter(line => /error TS\d+/.test(line))
    const seenMissing = new Set<string>()
    for (const line of errorLines) {
      const missing = line.match(/Cannot find module '([^']+)'/)
      const moduleName = missing?.[1]
      const known = moduleName ? EXPECTED_MISSING_MODULES[moduleName] : undefined
      if (moduleName && known) {
        if (seenMissing.has(moduleName)) continue
        seenMissing.add(moduleName)
        record({
          check: "typecheck",
          package: known.package,
          status: "fail",
          reason: `cannot find module '${moduleName}'`,
          evidence: line.trim(),
        })
      } else {
        record({
          check: "typecheck",
          package: "fixture",
          status: "fail",
          reason: "unexpected typecheck error",
          evidence: line.trim(),
        })
      }
    }
  }
}

// ---------------------------------------------------------------------------
// Step 7 — packed-contents check
// ---------------------------------------------------------------------------

function tarEntries(tarball: string): string[] {
  const listed = run("tar", ["-tzf", tarball], scriptDir)
  return listed.stdout
    .split("\n")
    .map(line => line.trim())
    .filter(line => line.length > 0)
}

// Intended publish set for every publishable package: manifest, docs, license,
// contract file, and built output. When a manifest carries a `files` allowlist,
// that allowlist (plus the files npm always includes) is the source of truth.
const DEFAULT_ALLOWED_PATTERNS: RegExp[] = [
  /^package\.json$/,
  /^README(\.|$)/i,
  /^LICENSE(\.|$)/i,
  /^PACKAGE_LAYOUT\.md$/,
  /^dist\//,
]

// Files OUTSIDE the intended publish set that sdk/core ship TODAY, because no
// publishable manifest carries a `files` allowlist yet. Listed explicitly — not
// a silent allowance: anything not named here fails the run. Tightening the
// manifests (adding `files` and dropping these entries) is owned by tasks 03/17.
const EXTRA_SHIPPED_TODAY: Record<string, RegExp[]> = {
  sdk: [/^AGENTS\.md$/, /^scripts\//, /^src\//, /^tsconfig\.json$/, /^tsdown\.config\.ts$/],
  core: [
    /^AGENTS\.md$/,
    /^kubb\.config\.ts$/,
    /^src\//,
    /^tsconfig\.json$/,
    /^tsdown\.config\.ts$/,
  ],
}

/** Escapes a string for literal use inside a RegExp. */
function escapeRegExp(text: string): string {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")
}

/** The exact-contents allowlist for one packed package. */
function allowedPatternsFor(name: string): RegExp[] {
  const manifest = JSON.parse(readFileSync(join(packagesDir, name, "package.json"), "utf8")) as {
    files?: string[]
  }

  if (manifest.files && manifest.files.length > 0) {
    // npm always includes package.json, README, and LICENSE regardless of `files`.
    const always = [/^package\.json$/, /^README(\.|$)/i, /^LICENSE(\.|$)/i]
    const fromFiles = manifest.files.map(entry => {
      const cleaned = entry.replace(/^\.\//, "").replace(/\/$/, "")
      return new RegExp(`^${escapeRegExp(cleaned)}(/|$)`)
    })
    return [...always, ...fromFiles]
  }

  return [...DEFAULT_ALLOWED_PATTERNS, ...(EXTRA_SHIPPED_TODAY[name] ?? [])]
}

for (const name of ["sdk", "core"]) {
  const tarball = tarballs.get(name)
  if (!tarball) continue
  const entries = tarEntries(tarball).map(entry => entry.replace(/^package\//, ""))

  // Required artifacts must be present…
  const hasManifest = entries.includes("package.json")
  const hasDist = entries.some(entry => /^dist\/.+\.js$/.test(entry))
  // Only the three contract packages ship a PACKAGE_LAYOUT.md; core does not.
  const requiresContract = existsSync(join(packagesDir, name, "PACKAGE_LAYOUT.md"))
  const hasContract = entries.includes("PACKAGE_LAYOUT.md")

  // …and nothing outside the exact allowlist may ship.
  const allowed = allowedPatternsFor(name)
  const unexpected = entries.filter(entry => !allowed.some(pattern => pattern.test(entry)))

  const problems: string[] = []
  if (!hasManifest) problems.push("missing package.json")
  if (!hasDist) problems.push("missing dist/*.js")
  if (requiresContract && !hasContract) problems.push("missing PACKAGE_LAYOUT.md")
  if (unexpected.length > 0) {
    const shown = unexpected.slice(0, 5).join(", ")
    problems.push(
      `ships ${unexpected.length} file(s) outside the expected set: ${shown}${unexpected.length > 5 ? ", …" : ""}`,
    )
  }

  record({
    check: "packed-contents",
    package: name,
    status: problems.length === 0 ? "pass" : "fail",
    reason: problems.length === 0 ? "ok" : problems.join("; "),
    evidence:
      problems.length === 0
        ? `${entries.length} entries, all within the expected file set${requiresContract ? " (incl. PACKAGE_LAYOUT.md)" : ""}`
        : `${entries.length} entries in ${relative(scriptDir, tarball)}`,
  })
}

// Per-platform prebuild package (@alienplatform/bindings-<platform>): not built
// until task 04a. Its packed shape (exactly one .node addon + manifest) cannot be
// asserted yet.
record({
  check: "packed-contents",
  package: "@alienplatform/bindings-darwin-arm64",
  status: "fail",
  reason: "per-platform prebuild package not present (expected one .node addon + manifest)",
  evidence: "no @alienplatform/bindings-darwin-arm64 tarball to inspect",
})

// ---------------------------------------------------------------------------
// Step 8 — bun build --compile of the ./native embed entry
// ---------------------------------------------------------------------------

if (bunAvailable) {
  const compiledDir = join(fixtureDir, ".compiled")
  mkdirSync(compiledDir, { recursive: true })
  const outFile = join(compiledDir, "compile-entry-bin")
  const built = run(
    "bun",
    ["build", "--compile", join("src", "compile-entry.ts"), "--outfile", outFile],
    fixtureDir,
  )
  if (built.status !== 0) {
    record({
      check: "compile",
      package: "bindings",
      status: "fail",
      reason: "bun build --compile of ./native entry fails (bindings package not installed)",
      evidence: lastLine(built.stderr) || lastLine(built.stdout) || `exit ${built.status}`,
    })
  } else {
    const ran = run(outFile, [], fixtureDir)
    record({
      check: "compile",
      package: "bindings",
      status: ran.status === 0 ? "pass" : "fail",
      reason: ran.status === 0 ? "ok" : "compiled binary exited non-zero",
      evidence: ran.status === 0 ? lastLine(ran.stdout) : lastLine(ran.stderr),
    })
  }
}

// ---------------------------------------------------------------------------
// Step 9 — invoke the static validator
// ---------------------------------------------------------------------------

const validator = run("node", ["--experimental-strip-types", validatorPath], scriptDir)
record({
  check: "validator",
  package: "layout",
  status: validator.status === 0 ? "pass" : "fail",
  reason:
    validator.status === 0
      ? "ok"
      : "packages/scripts validator reported unexpected failures or stale expectations",
  evidence: lastLine(validator.stdout) || lastLine(validator.stderr) || `exit ${validator.status}`,
})

// ---------------------------------------------------------------------------
// Reconcile against expected-failures.json and report
// ---------------------------------------------------------------------------

const expectedFailures = JSON.parse(
  readFileSync(join(scriptDir, "expected-failures.json"), "utf8"),
) as ExpectedFailureEntry[]

// The `bun build --compile` failure is only produced when bun runs. If bun is
// unavailable in this environment, drop that expectation so it is not counted
// stale (CI always has bun, so the committed list stays complete there).
const activeExpectations = bunAvailable
  ? expectedFailures
  : expectedFailures.filter(entry => !(entry.check === "compile" && entry.package === "bindings"))

const violations: Violation[] = results
  .filter(result => result.status === "fail")
  .map(result => ({
    check: result.check,
    package: result.package,
    reason: result.reason,
    evidence: result.evidence,
  }))

const filtered = applyExpectedFailures(violations, activeExpectations)

function expectationKey(entry: { check: string; package: string; reason: string }): string {
  return `${entry.check}::${entry.package}::${entry.reason}`
}
const expectedKeys = new Map(activeExpectations.map(entry => [expectationKey(entry), entry]))

console.log("")
console.log("package-layout fixture — step results")
console.log("=====================================")

let passCount = 0
let expectedCount = 0
let fatalCount = 0
for (const result of results) {
  if (result.status === "pass") {
    passCount += 1
    console.log(`  PASS       ${result.check} package=${result.package} — ${result.evidence}`)
    continue
  }
  const entry = expectedKeys.get(expectationKey(result))
  if (entry) {
    expectedCount += 1
    console.log(
      `  [expected] ${result.check} package=${result.package} (task ${entry.owningTask}): ${result.reason}`,
    )
    console.log(`             evidence: ${result.evidence}`)
  } else {
    fatalCount += 1
    console.error(`  FAIL       ${result.check} package=${result.package}: ${result.reason}`)
    console.error(`             evidence: ${result.evidence}`)
  }
}

for (const staleEntry of filtered.stale) {
  console.error(
    `  STALE      ${staleEntry.check} package=${staleEntry.package}: "${staleEntry.reason}" ` +
      `(task ${staleEntry.owningTask}) is listed in expected-failures.json but never occurred — remove it or fix the check.`,
  )
}

console.log("")
const exitCode = exitCodeFor(filtered)
if (exitCode === 0) {
  console.log(
    `OK: ${passCount} passed, ${expectedCount} expected failure(s), 0 unexpected, 0 stale.`,
  )
} else {
  console.log(
    `FAILED: ${passCount} passed, ${fatalCount} unexpected failure(s), ${filtered.stale.length} stale expectation(s).`,
  )
}

process.exit(exitCode)
