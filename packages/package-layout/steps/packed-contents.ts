// Packed-contents check for every publishable package tarball.
//
// Asserts each packed tarball ships exactly its intended publish set: required
// artifacts present, nothing outside the allowlist, no hard-denylisted files.
// Per-platform prebuild packages are built, architecture-checked, installed,
// and smoke-tested by the native release matrix; they do not exist in a normal
// source checkout and are therefore not represented as expected failures here.

import { readFileSync } from "node:fs"
import { join, relative } from "node:path"
import { type CheckResult, type Ctx, HARD_DENYLIST_PATTERNS, escapeRegExp, run } from "./shared.ts"

/** List the entries of a packed tarball (paths, `package/`-prefixed). */
export function tarEntries(tarball: string, cwd: string): string[] {
  const listed = run("tar", ["-tzf", tarball], cwd)
  return listed.stdout
    .split("\n")
    .map(line => line.trim())
    .filter(line => line.length > 0)
}

export function packedContents(ctx: Ctx): CheckResult[] {
  const { scriptDir, packagesDir, tarballs } = ctx
  const results: CheckResult[] = []

  // Intended publish set for every publishable package: manifest, docs, license,
  // contract file, and built output. When a manifest carries a `files` allowlist,
  // that allowlist (plus the files npm always includes) is the source of truth.
  const DEFAULT_ALLOWED_PATTERNS: RegExp[] = [
    /^package\.json$/,
    /^README(\.|$)/i,
    /^LICENSE(\.|$)/i,
    /^dist\//,
  ]

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

    return DEFAULT_ALLOWED_PATTERNS
  }

  for (const name of ["sdk", "core", "bindings", "commands"]) {
    const tarball = tarballs.get(name)
    if (!tarball) continue
    const entries = tarEntries(tarball, scriptDir).map(entry => entry.replace(/^package\//, ""))

    // Required artifacts must be present…
    const hasManifest = entries.includes("package.json")
    const hasDist = entries.some(entry => /^dist\/.+\.js$/.test(entry))

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

    results.push({
      check: "packed-contents",
      package: name,
      status: problems.length === 0 ? "pass" : "fail",
      reason: problems.length === 0 ? "ok" : problems.join("; "),
      evidence:
        problems.length === 0
          ? `${entries.length} entries, all within the expected file set`
          : `${entries.length} entries in ${relative(scriptDir, tarball)}`,
    })
  }

  return results
}
