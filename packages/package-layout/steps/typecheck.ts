// Step 6 — tsc typecheck of the consumer.

import { existsSync } from "node:fs"
import { join } from "node:path"
import { type CheckResult, type Ctx, run } from "./shared.ts"

export function typecheckConsumer(ctx: Ctx): CheckResult[] {
  const { fixtureDir } = ctx

  const tscBin = join(fixtureDir, "node_modules", "typescript", "bin", "tsc")
  if (!existsSync(tscBin)) {
    return [
      {
        check: "typecheck",
        package: "fixture",
        status: "fail",
        reason: "typescript is not installed in the consumer (install step failed?)",
        evidence: tscBin,
      },
    ]
  }

  const tsc = run("node", [tscBin, "--noEmit", "-p", "tsconfig.json"], fixtureDir)
  if (tsc.status === 0) {
    return [
      {
        check: "typecheck",
        package: "fixture",
        status: "pass",
        reason: "ok",
        evidence: "tsc --noEmit reported no errors",
      },
    ]
  }

  const errorLines = tsc.stdout.split("\n").filter(line => /error TS\d+/.test(line))
  return errorLines.map(line => ({
    check: "typecheck",
    package: "fixture",
    status: "fail" as const,
    reason: "unexpected typecheck error",
    evidence: line.trim(),
  }))
}
