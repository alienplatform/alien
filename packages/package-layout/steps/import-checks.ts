// Steps 4/5 — import check under Bun and Node.

import { join } from "node:path"
import { type CheckResult, type Ctx, lastLine, run } from "./shared.ts"

export function runImportChecks(ctx: Ctx): CheckResult[] {
  const { fixtureDir, bunAvailable, addonEnv } = ctx
  const results: CheckResult[] = []
  const IMPORTS_ENTRY = join("src", "imports.ts")

  function runImportCheck(runtime: "bun" | "node"): void {
    const output =
      runtime === "bun"
        ? run("bun", [IMPORTS_ENTRY], fixtureDir, addonEnv)
        : run("node", ["--experimental-strip-types", IMPORTS_ENTRY], fixtureDir, addonEnv)

    const lines = output.stdout.split("\n").filter(line => line.startsWith("##CHECK## "))
    if (lines.length === 0) {
      results.push({
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
      results.push({
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

  return results
}
