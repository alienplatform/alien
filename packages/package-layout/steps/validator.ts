// Step 9 — invoke the static validator (packages/scripts/validate-package-layout.ts).

import { type CheckResult, type Ctx, lastLine, run } from "./shared.ts"

export function runValidator(ctx: Ctx): CheckResult[] {
  const validator = run("node", ["--experimental-strip-types", ctx.validatorPath], ctx.scriptDir)
  return [
    {
      check: "validator",
      package: "layout",
      status: validator.status === 0 ? "pass" : "fail",
      reason:
        validator.status === 0
          ? "ok"
          : "packages/scripts validator reported unexpected failures or stale expectations",
      evidence:
        lastLine(validator.stdout) || lastLine(validator.stderr) || `exit ${validator.status}`,
    },
  ]
}
