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
 * and zero stale expectations (reused applyExpectedFailures + exitCodeFor), and
 * zero cross-runtime divergences (see `detectRuntimeDivergence` below).
 *
 * Only pure, side-effect-free declarations run at module load (types, helper
 * functions, `detectRuntimeDivergence`). Everything that touches the filesystem
 * or spawns a process lives in `main()`, invoked only when this file is the
 * program entry point — so importing this module (e.g. from a unit test) never
 * triggers pack/install/import/compile or calls `process.exit`.
 */

import { spawnSync } from "node:child_process"
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from "node:fs"
import { createRequire } from "node:module"
import { dirname, join, relative } from "node:path"
import { fileURLToPath } from "node:url"
import {
  type ExpectedFailureEntry,
  type Violation,
  applyExpectedFailures,
  exitCodeFor,
} from "../scripts/validate-package-layout.ts"

/** One reported check. Failing ones are reconciled against expected-failures.json. */
interface CheckResult {
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

interface RunOutput {
  status: number | null
  stdout: string
  stderr: string
}

function run(command: string, args: string[], cwd: string, env?: NodeJS.ProcessEnv): RunOutput {
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

function lastLine(text: string): string {
  const lines = text
    .split("\n")
    .map(line => line.trim())
    .filter(line => line.length > 0)
  return lines.at(-1) ?? ""
}

/** Escapes a string for literal use inside a RegExp. */
function escapeRegExp(text: string): string {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")
}

function isMainModule(): boolean {
  return typeof process.argv[1] === "string" && import.meta.url === `file://${process.argv[1]}`
}

// ---------------------------------------------------------------------------
// Everything below is imperative: filesystem I/O, subprocess spawning, and
// process.exit. Gated behind main() so importing this module is side-effect
// free (see the file-level doc comment above).
// ---------------------------------------------------------------------------

function main(): void {
  const scriptDir = dirname(fileURLToPath(import.meta.url))
  const packagesDir = join(scriptDir, "..")
  const tarballsDir = join(scriptDir, ".tarballs")
  const fixtureDir = join(scriptDir, "fixture")
  const validatorPath = join(packagesDir, "scripts", "validate-package-layout.ts")

  const results: CheckResult[] = []
  function record(result: CheckResult): void {
    results.push(result)
  }

  // -------------------------------------------------------------------------
  // Environment
  // -------------------------------------------------------------------------

  const bunAvailable = run("bun", ["--version"], scriptDir).status === 0
  if (!bunAvailable) {
    console.log(
      "[env-skip] bun is not on PATH — the Bun import and `bun build --compile` steps are " +
        "skipped in this environment. CI provides bun via setup-bun.",
    )
  }

  // -------------------------------------------------------------------------
  // Step 1 — pack the publishable packages
  // -------------------------------------------------------------------------

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

  // -------------------------------------------------------------------------
  // Step 2 — rewrite the consumer manifest to file: the tarballs
  // -------------------------------------------------------------------------

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

  // -------------------------------------------------------------------------
  // Step 3 — npm install the consumer, assert tarball resolution
  // -------------------------------------------------------------------------

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

  // -------------------------------------------------------------------------
  // Step 3.5 — ensure a native addon exists for the host platform
  // -------------------------------------------------------------------------
  //
  // The loader (packages/bindings/src/loader.ts) resolves the addon in order:
  // an ALIEN_BINDINGS_ADDON_PATH override, the per-platform prebuild package
  // (optionalDependencies — only injected at publish time by `napi
  // prepublish`, task 04a; never present when packing straight from workspace
  // source), then a locally-built dev `.node` found by walking up from the
  // installed package looking for crates/alien-bindings-node. On a developer
  // machine that walk reaches this repo's real crates/alien-bindings-node and
  // finds the `.node` a developer built earlier, so the fixture passes
  // without any help here. CI has neither: no prebuild (04a ships those) and
  // no dev `.node` (gitignored, built per-machine) — so build one ourselves
  // whenever nothing else would resolve, and hand its path to every
  // subprocess via the override env var. Skipped (and logged) whenever an
  // addon is already available, so local runs stay fast.

  function hostTriple(): string {
    const { platform, arch } = process
    if (platform === "darwin" && arch === "arm64") return "darwin-arm64"
    if (platform === "darwin" && arch === "x64") return "darwin-x64"
    if (platform === "linux" && arch === "x64") return "linux-x64-gnu"
    if (platform === "linux" && arch === "arm64") return "linux-arm64-gnu"
    throw new Error(
      `package-layout fixture has no native addon mapping for platform '${platform}' arch '${arch}'.`,
    )
  }

  const repoRoot = dirname(packagesDir)
  const bindingsNodeDir = join(repoRoot, "crates", "alien-bindings-node")
  const triple = hostTriple()
  const devAddonPath = join(bindingsNodeDir, `alien-bindings-node.${triple}.node`)
  const prebuildInstalledDir = join(
    fixtureDir,
    "node_modules",
    "@alienplatform",
    `bindings-${triple}`,
  )

  let addonPath: string | undefined
  if (existsSync(prebuildInstalledDir)) {
    console.log(
      `[addon] per-platform prebuild package installed for '${triple}' — no source build needed.`,
    )
  } else if (existsSync(devAddonPath)) {
    addonPath = devAddonPath
    console.log(
      `[addon] using existing dev addon at ${relative(scriptDir, devAddonPath)} (fast path, no build).`,
    )
  } else {
    console.log(
      `[addon] no prebuild and no dev addon for '${triple}' — building one with \`napi build --platform --release\` in crates/alien-bindings-node (CI path)...`,
    )
    // crates/alien-bindings-node is not a pnpm workspace member, so it has no
    // node_modules in CI — `npx napi` there would fall through to the npm
    // registry. Resolve the napi CLI from this package's own devDependencies
    // (installed by the root frozen-lockfile `pnpm install`) and spawn its bin
    // directly with cwd set to the crate dir.
    const napiRequire = createRequire(import.meta.url)
    const napiPkgPath = napiRequire.resolve("@napi-rs/cli/package.json")
    const napiPkg = JSON.parse(readFileSync(napiPkgPath, "utf8")) as {
      bin: string | Record<string, string>
    }
    const napiBinRel = typeof napiPkg.bin === "string" ? napiPkg.bin : napiPkg.bin.napi
    const napiBinPath = join(dirname(napiPkgPath), napiBinRel)
    const build = run(
      process.execPath,
      [napiBinPath, "build", "--platform", "--release"],
      bindingsNodeDir,
    )
    if (build.status === 0 && existsSync(devAddonPath)) {
      addonPath = devAddonPath
      console.log(`[addon] built ${relative(scriptDir, devAddonPath)}.`)
    } else {
      console.error(
        "[addon] source build failed; the runtime/compile checks below will fail to load the addon.",
      )
      record({
        check: "addon-build",
        package: "bindings",
        status: "fail",
        reason: "napi build --platform --release did not produce a .node for this host",
        evidence: lastLine(build.stderr) || lastLine(build.stdout) || `exit ${build.status}`,
      })
    }
  }

  const addonEnv: NodeJS.ProcessEnv | undefined = addonPath
    ? { ALIEN_BINDINGS_ADDON_PATH: addonPath }
    : undefined

  // -------------------------------------------------------------------------
  // Steps 4/5 — import check under Bun and Node
  // -------------------------------------------------------------------------

  const IMPORTS_ENTRY = join("src", "imports.ts")

  function runImportCheck(runtime: "bun" | "node"): void {
    const output =
      runtime === "bun"
        ? run("bun", [IMPORTS_ENTRY], fixtureDir, addonEnv)
        : run("node", ["--experimental-strip-types", IMPORTS_ENTRY], fixtureDir, addonEnv)

    const lines = output.stdout.split("\n").filter(line => line.startsWith("##CHECK## "))
    if (lines.length === 0) {
      record({
        check: `${runtime}-imports`,
        package: "fixture",
        status: "fail",
        reason: "import check produced no results (crashed before reporting)",
        evidence: lastLine(output.stderr) || lastLine(output.stdout) || `exit ${output.status}`,
        runtime,
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
        runtime,
      })
    }
  }

  if (bunAvailable) runImportCheck("bun")
  runImportCheck("node")

  // -------------------------------------------------------------------------
  // Step 6 — tsc typecheck of the consumer
  // -------------------------------------------------------------------------

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

  // -------------------------------------------------------------------------
  // Step 7 — packed-contents check
  // -------------------------------------------------------------------------

  function tarEntries(tarball: string): string[] {
    const listed = run("tar", ["-tzf", tarball], scriptDir)
    return listed.stdout
      .split("\n")
      .map(line => line.trim())
      .filter(line => line.length > 0)
  }

  // Hard denylist: never allowed in a packed tarball, regardless of `files` or
  // EXTRA_SHIPPED_TODAY. Catches regressions even if an .npmignore is deleted.
  const HARD_DENYLIST_PATTERNS: RegExp[] = [
    /(^|\/)node_modules\//,
    /(^|\/)\.env$/,
    /\.tgz$/,
    /(^|\/)\.turbo\//,
  ]

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

    // …and nothing outside the exact allowlist may ship. Hard-denylisted entries
    // are reported separately (and always), so exclude them from `unexpected` to
    // avoid reporting the same file twice.
    const denylisted = entries.filter(entry =>
      HARD_DENYLIST_PATTERNS.some(pattern => pattern.test(entry)),
    )
    const allowed = allowedPatternsFor(name)
    const unexpected = entries.filter(
      entry => !denylisted.includes(entry) && !allowed.some(pattern => pattern.test(entry)),
    )

    const problems: string[] = []
    if (!hasManifest) problems.push("missing package.json")
    if (!hasDist) problems.push("missing dist/*.js")
    if (requiresContract && !hasContract) problems.push("missing PACKAGE_LAYOUT.md")
    if (denylisted.length > 0) {
      const shown = denylisted.slice(0, 5).join(", ")
      problems.push(
        `ships ${denylisted.length} hard-denylisted file(s): ${shown}${denylisted.length > 5 ? ", …" : ""}`,
      )
    }
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

  // Per-platform prebuild packages (@alienplatform/bindings-<triple>): assert the
  // packed shape — exactly one `.node` addon (named by the manifest `main`) plus
  // its manifest, with os/cpu set — for every triple whose addon is staged in
  // packages/bindings/npm/<triple>. Each addon is built by the release pipeline
  // on a native runner (napi artifacts stages it into these dirs at publish
  // time), so a plain workspace checkout has no addon for any triple and every
  // one is recorded as an expected failure naming where it is produced. That set
  // is host-independent — this fixture never stages the locally-built dev addon
  // into an npm dir (the dev addon serves the import/compile checks via
  // ALIEN_BINDINGS_ADDON_PATH instead) — so the committed expected-failures.json
  // reconciles identically on the darwin dev host and the linux-arm64 CI runner.
  // The inspection branch below is exercised for real whenever an addon IS staged
  // (a release-pipeline dry run, or `bun run build:addon` followed by a manual
  // stage), and was proven locally by packing the darwin-arm64 + darwin-x64 dirs.
  const PREBUILD_TRIPLES = [
    "darwin-arm64",
    "darwin-x64",
    "linux-x64-gnu",
    "linux-arm64-gnu",
  ] as const
  const RELEASE_MATRIX_NOTE: Record<string, string> = {
    "darwin-arm64": "built natively on the macOS runner",
    "darwin-x64": "cross-compiled on the macOS runner via --target x86_64-apple-darwin",
    "linux-x64-gnu": "built natively on the linux-x64 runner",
    "linux-arm64-gnu": "built natively on the linux-arm64 runner",
  }
  const bindingsNpmDir = join(packagesDir, "bindings", "npm")

  interface PrebuildManifest {
    name: string
    main: string
    os?: string[]
    cpu?: string[]
    libc?: string[]
  }

  for (const prebuildTriple of PREBUILD_TRIPLES) {
    const pkgName = `@alienplatform/bindings-${prebuildTriple}`
    const npmDir = join(bindingsNpmDir, prebuildTriple)
    const manifestPath = join(npmDir, "package.json")
    if (!existsSync(manifestPath)) {
      record({
        check: "packed-contents",
        package: pkgName,
        status: "fail",
        reason: `npm skeleton dir missing (${relative(scriptDir, npmDir)})`,
        evidence: npmDir,
      })
      continue
    }
    const manifest = JSON.parse(readFileSync(manifestPath, "utf8")) as PrebuildManifest
    const stagedAddon = join(npmDir, manifest.main)
    if (!existsSync(stagedAddon)) {
      record({
        check: "packed-contents",
        package: pkgName,
        status: "fail",
        reason: `per-platform prebuild addon not staged locally (release matrix: ${RELEASE_MATRIX_NOTE[prebuildTriple]})`,
        evidence: `no ${manifest.main} in ${relative(scriptDir, npmDir)} — built and published by the release pipeline`,
      })
      continue
    }

    const packed = run("npm", ["pack", "--pack-destination", tarballsDir, "--silent"], npmDir)
    const version = (JSON.parse(readFileSync(manifestPath, "utf8")) as { version?: string }).version
    const tarballName = readdirSync(tarballsDir).find(
      entry =>
        entry.startsWith(`alienplatform-bindings-${prebuildTriple}-`) && entry.endsWith(".tgz"),
    )
    if (packed.status !== 0 || !tarballName) {
      record({
        check: "packed-contents",
        package: pkgName,
        status: "fail",
        reason: "npm pack failed",
        evidence: lastLine(packed.stderr) || lastLine(packed.stdout) || `exit ${packed.status}`,
      })
      continue
    }
    const entries = tarEntries(join(tarballsDir, tarballName)).map(entry =>
      entry.replace(/^package\//, ""),
    )
    const nodeFiles = entries.filter(entry => entry.endsWith(".node"))

    const problems: string[] = []
    if (!entries.includes("package.json")) problems.push("missing package.json")
    if (nodeFiles.length !== 1) {
      problems.push(`expected exactly one .node addon, found ${nodeFiles.length}`)
    } else if (nodeFiles[0] !== manifest.main) {
      problems.push(`.node addon '${nodeFiles[0]}' does not match manifest main '${manifest.main}'`)
    }
    if (manifest.name !== pkgName) {
      problems.push(`manifest name '${manifest.name}' does not match '${pkgName}'`)
    }
    if (!manifest.os || manifest.os.length === 0) problems.push("manifest missing os")
    if (!manifest.cpu || manifest.cpu.length === 0) problems.push("manifest missing cpu")
    const denylisted = entries.filter(entry =>
      HARD_DENYLIST_PATTERNS.some(pattern => pattern.test(entry)),
    )
    if (denylisted.length > 0) {
      problems.push(`ships hard-denylisted file(s): ${denylisted.slice(0, 5).join(", ")}`)
    }

    record({
      check: "packed-contents",
      package: pkgName,
      status: problems.length === 0 ? "pass" : "fail",
      reason: problems.length === 0 ? "ok" : problems.join("; "),
      evidence:
        problems.length === 0
          ? `${entries.length} entries in ${tarballName}: exactly one .node (${nodeFiles[0]}) + manifest (version=${version}, os=${manifest.os?.join(",")}, cpu=${manifest.cpu?.join(",")}${manifest.libc ? `, libc=${manifest.libc.join(",")}` : ""})`
          : `${entries.length} entries in ${tarballName}`,
    })
  }

  // -------------------------------------------------------------------------
  // Step 8 — bun build --compile of the ./native embed entry
  // -------------------------------------------------------------------------
  //
  // The `./native` entry (bindings/src/native.ts) imports the addon through
  // the literal `./alien-bindings.node` specifier so bun's compiler can stage
  // it into the single-file binary — but only if that file is physically
  // present next to the installed package's dist/native.js at build time
  // (the staging contract task 13 owns in production; here we stage the
  // addon `run.ts` itself resolved above). `--format=cjs` is required: a
  // plain ESM `bun build --compile` of this entry embeds the addon but
  // crashes on load with `ReferenceError: __require is not defined` — see
  // packages/bindings/scripts/compile-smoke.ts for the verified repro.

  if (bunAvailable) {
    const compiledDir = join(fixtureDir, ".compiled")
    mkdirSync(compiledDir, { recursive: true })
    const outFile = join(compiledDir, "compile-entry-bin")
    const stagedAddonPath = join(
      fixtureDir,
      "node_modules",
      "@alienplatform",
      "bindings",
      "dist",
      "alien-bindings.node",
    )

    if (addonPath) copyFileSync(addonPath, stagedAddonPath)

    const built = addonPath
      ? run(
          "bun",
          [
            "build",
            "--compile",
            "--format=cjs",
            join("src", "compile-entry.ts"),
            "--outfile",
            outFile,
          ],
          fixtureDir,
        )
      : undefined

    if (!built) {
      record({
        check: "compile",
        package: "bindings",
        status: "fail",
        reason: "no addon available to stage (see the addon-build failure above)",
        evidence: `expected addon at ${stagedAddonPath}`,
      })
    } else if (built.status !== 0) {
      record({
        check: "compile",
        package: "bindings",
        status: "fail",
        reason: "bun build --compile of ./native entry fails with the addon staged",
        evidence: lastLine(built.stderr) || lastLine(built.stdout) || `exit ${built.status}`,
      })
    } else {
      // Remove the staged .node now: if the binary didn't truly embed it,
      // running with the source file gone proves that (mirrors
      // packages/bindings/scripts/compile-smoke.ts).
      rmSync(stagedAddonPath, { force: true })
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

  // -------------------------------------------------------------------------
  // Step 9 — invoke the static validator
  // -------------------------------------------------------------------------

  const validator = run("node", ["--experimental-strip-types", validatorPath], scriptDir)
  record({
    check: "validator",
    package: "layout",
    status: validator.status === 0 ? "pass" : "fail",
    reason:
      validator.status === 0
        ? "ok"
        : "packages/scripts validator reported unexpected failures or stale expectations",
    evidence:
      lastLine(validator.stdout) || lastLine(validator.stderr) || `exit ${validator.status}`,
  })

  // -------------------------------------------------------------------------
  // Reconcile against expected-failures.json and report
  // -------------------------------------------------------------------------

  const expectedFailures = JSON.parse(
    readFileSync(join(scriptDir, "expected-failures.json"), "utf8"),
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
}

if (isMainModule()) {
  main()
}
