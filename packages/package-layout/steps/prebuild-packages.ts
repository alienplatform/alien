// Step 7 (continued) — packed-contents check for the per-platform prebuild
// packages (@alienplatform/bindings-<triple>).
//
// Assert the packed shape — exactly one `.node` addon (named by the manifest
// `main`) plus its manifest, with os/cpu set — for every triple whose addon is
// staged in packages/bindings/npm/<triple>. Each addon is built by the release
// pipeline on a native runner (napi artifacts stages it into these dirs at
// publish time), so a plain workspace checkout has no addon for any triple and
// every one is recorded as an expected failure naming where it is produced. That
// set is host-independent — this fixture never stages the locally-built dev addon
// into an npm dir (the dev addon serves the import/compile checks via
// ALIEN_BINDINGS_ADDON_PATH instead) — so the committed expected-failures.json
// reconciles identically on the darwin dev host and the linux-arm64 CI runner.
// The inspection branch below is exercised for real whenever an addon IS staged
// (a release-pipeline dry run, or `bun run build:addon` followed by a manual
// stage), and was proven locally by packing the darwin-arm64 + darwin-x64 dirs.

import { existsSync, readFileSync, readdirSync } from "node:fs"
import { join, relative } from "node:path"
import { tarEntries } from "./packed-contents.ts"
import { type CheckResult, type Ctx, HARD_DENYLIST_PATTERNS, lastLine, run } from "./shared.ts"

const PREBUILD_TRIPLES = ["darwin-arm64", "darwin-x64", "linux-x64-gnu", "linux-arm64-gnu"] as const

const RELEASE_MATRIX_NOTE: Record<string, string> = {
  "darwin-arm64": "built natively on the macOS runner",
  "darwin-x64": "cross-compiled on the macOS runner via --target x86_64-apple-darwin",
  "linux-x64-gnu": "built natively on the linux-x64 runner",
  "linux-arm64-gnu": "built natively on the linux-arm64 runner",
}

interface PrebuildManifest {
  name: string
  main: string
  version?: string
  os?: string[]
  cpu?: string[]
  libc?: string[]
}

export function prebuildPackages(ctx: Ctx): CheckResult[] {
  const { scriptDir, packagesDir, tarballsDir } = ctx
  const results: CheckResult[] = []
  const bindingsNpmDir = join(packagesDir, "bindings", "npm")

  for (const prebuildTriple of PREBUILD_TRIPLES) {
    const pkgName = `@alienplatform/bindings-${prebuildTriple}`
    const npmDir = join(bindingsNpmDir, prebuildTriple)
    const manifestPath = join(npmDir, "package.json")
    if (!existsSync(manifestPath)) {
      results.push({
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
      results.push({
        check: "packed-contents",
        package: pkgName,
        status: "fail",
        reason: `per-platform prebuild addon not staged locally (release matrix: ${RELEASE_MATRIX_NOTE[prebuildTriple]})`,
        evidence: `no ${manifest.main} in ${relative(scriptDir, npmDir)} — built and published by the release pipeline`,
      })
      continue
    }

    const packed = run("npm", ["pack", "--pack-destination", tarballsDir, "--silent"], npmDir)
    const version = manifest.version
    const tarballName = readdirSync(tarballsDir).find(
      entry =>
        entry.startsWith(`alienplatform-bindings-${prebuildTriple}-`) && entry.endsWith(".tgz"),
    )
    if (packed.status !== 0 || !tarballName) {
      results.push({
        check: "packed-contents",
        package: pkgName,
        status: "fail",
        reason: "npm pack failed",
        evidence: lastLine(packed.stderr) || lastLine(packed.stdout) || `exit ${packed.status}`,
      })
      continue
    }
    const entries = tarEntries(join(tarballsDir, tarballName), scriptDir).map(entry =>
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

    results.push({
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

  return results
}
