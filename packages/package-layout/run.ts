/**
 * Package-layout fixture orchestrator.
 *
 * Runs the executable proof that the packed publishable packages install and
 * import correctly on Bun and Node, honoring the pinned surfaces in
 * packages/{sdk,bindings,commands}/PACKAGE_LAYOUT.md. Discrete steps:
 *
 *   1. pnpm pack each publishable package (sdk, core, bindings, commands)
 *      into .tarballs/.
 *   2. Rewrite the consumer's `dependencies` + `overrides` to file: those tarballs
 *      (every transitive @alienplatform/* pinned to a tarball, never npm).
 *   3. npm install the consumer (npm, not pnpm — a real consumer) and assert the
 *      @alienplatform/* packages resolved to the tarballs.
 *   4/5. Import check under Bun and Node (src/imports.ts) — pinned surfaces +
 *      BINDING_NOT_CONFIGURED / COMMAND_RECEIVER_CONFIG_INVALID by `code`.
 *   6. tsc typecheck of the consumer.
 *   7. packed-contents check (tar -tzf) of each tarball, plus the per-platform
 *      prebuild packages.
 *   8. bun build --compile of src/compile-entry.ts (the ./native embed) + run it.
 *   9. Invoke the static validator (packages/scripts/validate-package-layout.ts).
 *
 * Each step lives in its own `steps/*.ts` file as a `(ctx: Ctx) => CheckResult[]`
 * function; this file threads the shared `Ctx` through them in order (`main`) and
 * reconciles the concatenated results. Every step is individually reported (name,
 * PASS/[expected]/FAIL, evidence). Failing checks are reconciled against
 * expected-failures.json exactly like the validator does — the run exits 0 only
 * when there are zero unexpected failures and zero stale expectations (reused
 * applyExpectedFailures + exitCodeFor), and zero cross-runtime divergences (see
 * `detectRuntimeDivergence` below).
 *
 * Only pure, side-effect-free declarations run at module load (types, helper
 * functions, `detectRuntimeDivergence`). Everything that touches the filesystem
 * or spawns a process lives in the step functions, invoked only from `main()`
 * when this file is the program entry point — so importing this module (e.g. from
 * a unit test) never triggers pack/install/import/compile or calls `process.exit`.
 */

import { readFileSync } from "node:fs"
import { dirname, join } from "node:path"
import { fileURLToPath } from "node:url"
import {
  type ExpectedFailureEntry,
  type Violation,
  applyExpectedFailures,
  exitCodeFor,
  expectedFailureKey,
  isMainModule,
} from "../scripts/validate-package-layout.ts"
import { compileNativeEmbed } from "./steps/compile.ts"
import { ensureAddon } from "./steps/ensure-addon.ts"
import { runImportChecks } from "./steps/import-checks.ts"
import { installConsumer } from "./steps/install-consumer.ts"
import { packPackages } from "./steps/pack-packages.ts"
import { packedContents } from "./steps/packed-contents.ts"
import { prebuildPackages } from "./steps/prebuild-packages.ts"
import { type CheckResult, type Ctx, run } from "./steps/shared.ts"
import { typecheckConsumer } from "./steps/typecheck.ts"
import { runValidator } from "./steps/validator.ts"
import { writeManifest } from "./steps/write-manifest.ts"

/**
 * Flags a check+package pair that passes on one JS runtime and fails on the
 * other. This is the one case `applyExpectedFailures` cannot see on its own:
 * that function's key is `check::package::reason`, and a runtime is only
 * recorded in `evidence` — so if Bun and Node happen to report the same
 * check/package/reason (e.g. because a still-registered expected-failure
 * entry describes "not implemented yet" text that both runtimes emit while
 * the underlying package doesn't exist), a fix landing on only one runtime
 * would still fully satisfy that shared expectation and the run would exit 0.
 * Divergence violations always use the `runtime-divergence` check name, which
 * never appears in expected-failures.json, so `applyExpectedFailures` can
 * never classify one as expected — it is unconditionally fatal.
 *
 * Pure function (no I/O) so it can be unit-tested directly against crafted
 * result lists instead of only through the full pack/install/import pipeline.
 */
export function detectRuntimeDivergence(results: readonly CheckResult[]): Violation[] {
  const byKey = new Map<string, Map<"bun" | "node", CheckResult>>()

  for (const result of results) {
    if (!result.runtime) continue
    const key = `${result.check}::${result.package}`
    const byRuntime = byKey.get(key) ?? new Map<"bun" | "node", CheckResult>()
    byRuntime.set(result.runtime, result)
    byKey.set(key, byRuntime)
  }

  const violations: Violation[] = []
  for (const byRuntime of byKey.values()) {
    const bun = byRuntime.get("bun")
    const node = byRuntime.get("node")
    if (!bun || !node || bun.status === node.status) continue

    const passing = bun.status === "pass" ? bun : node
    const failing = bun.status === "pass" ? node : bun
    violations.push({
      check: "runtime-divergence",
      package: passing.package,
      reason: `${passing.check} passes on ${passing.runtime} but fails on ${failing.runtime}`,
      evidence: `${passing.runtime}: ${passing.evidence} | ${failing.runtime}: ${failing.evidence}`,
    })
  }

  return violations
}

function createContext(): Ctx {
  const scriptDir = dirname(fileURLToPath(import.meta.url))
  const packagesDir = join(scriptDir, "..")

  const bunAvailable = run("bun", ["--version"], scriptDir).status === 0
  if (!bunAvailable) {
    console.log(
      "[env-skip] bun is not on PATH — the Bun import and `bun build --compile` steps are " +
        "skipped in this environment. CI provides bun via setup-bun.",
    )
  }

  return {
    scriptDir,
    packagesDir,
    tarballsDir: join(scriptDir, ".tarballs"),
    fixtureDir: join(scriptDir, "fixture"),
    repoRoot: dirname(packagesDir),
    validatorPath: join(packagesDir, "scripts", "validate-package-layout.ts"),
    bunAvailable,
    tarballs: new Map<string, string>(),
  }
}

// -------------------------------------------------------------------------
// Reconcile against expected-failures.json and report
// -------------------------------------------------------------------------

function reconcileAndReport(ctx: Ctx, results: CheckResult[]): number {
  const expectedFailures = JSON.parse(
    readFileSync(join(ctx.scriptDir, "expected-failures.json"), "utf8"),
  ) as ExpectedFailureEntry[]

  // Every currently-expected failure applies regardless of runtime — the one
  // entry that depended on `bunAvailable` (the `compile` check itself never
  // running without bun) was removed once the fixture started staging a
  // built addon and passing that check. Keep the name so a future
  // bun-conditional expectation has an obvious place to slot back in.
  const activeExpectations = expectedFailures

  // Computed from ALL results (pass and fail) — a divergence is defined by one
  // runtime passing while the other fails, so passes must stay in view.
  const divergences = detectRuntimeDivergence(results)

  const violations: Violation[] = [
    ...results
      .filter(result => result.status === "fail")
      .map(result => ({
        check: result.check,
        package: result.package,
        reason: result.reason,
        evidence: result.evidence,
      })),
    // Always fatal: "runtime-divergence" is never a key in expected-failures.json,
    // so applyExpectedFailures can only ever bucket these as unexpected.
    ...divergences,
  ]

  const filtered = applyExpectedFailures(violations, activeExpectations)

  const expectedKeys = new Map(activeExpectations.map(entry => [expectedFailureKey(entry), entry]))

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
    const entry = expectedKeys.get(expectedFailureKey(result))
    if (entry) {
      expectedCount += 1
      console.log(
        `  [expected] ${result.check} package=${result.package} (owner: ${entry.owningTask}): ${result.reason}`,
      )
      console.log(`             evidence: ${result.evidence}`)
    } else {
      fatalCount += 1
      console.error(`  FAIL       ${result.check} package=${result.package}: ${result.reason}`)
      console.error(`             evidence: ${result.evidence}`)
    }
  }

  for (const divergence of divergences) {
    fatalCount += 1
    console.error(
      `  FAIL       ${divergence.check} package=${divergence.package}: ${divergence.reason}`,
    )
    console.error(`             evidence: ${divergence.evidence}`)
  }

  for (const staleEntry of filtered.stale) {
    console.error(
      `  STALE      ${staleEntry.check} package=${staleEntry.package}: "${staleEntry.reason}" ` +
        `(owner: ${staleEntry.owningTask}) is listed in expected-failures.json but never occurred — remove it or fix the check.`,
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

  return exitCode
}

function main(): void {
  const ctx = createContext()

  const results: CheckResult[] = [
    ...packPackages(ctx),
    ...writeManifest(ctx),
    ...installConsumer(ctx),
    ...ensureAddon(ctx),
    ...runImportChecks(ctx),
    ...typecheckConsumer(ctx),
    ...packedContents(ctx),
    ...prebuildPackages(ctx),
    ...compileNativeEmbed(ctx),
    ...runValidator(ctx),
  ]

  process.exit(reconcileAndReport(ctx, results))
}

if (isMainModule(import.meta.url)) {
  main()
}
