import { describe, expect, it } from "vitest"

import { detectRuntimeDivergence } from "./run.ts"

describe("detectRuntimeDivergence", () => {
  it("reports nothing when both runtimes agree (both pass)", () => {
    const violations = detectRuntimeDivergence([
      {
        check: "import",
        package: "sdk",
        status: "pass",
        reason: "ok",
        evidence: "a",
        runtime: "bun",
      },
      {
        check: "import",
        package: "sdk",
        status: "pass",
        reason: "ok",
        evidence: "b",
        runtime: "node",
      },
    ])

    expect(violations).toHaveLength(0)
  })

  it("reports nothing when both runtimes agree (both fail)", () => {
    const violations = detectRuntimeDivergence([
      {
        check: "import",
        package: "bindings",
        status: "fail",
        reason: "cannot import @alienplatform/bindings (package not installed)",
        evidence: "bun evidence",
        runtime: "bun",
      },
      {
        check: "import",
        package: "bindings",
        status: "fail",
        reason: "cannot import @alienplatform/bindings (package not installed)",
        evidence: "node evidence",
        runtime: "node",
      },
    ])

    expect(violations).toHaveLength(0)
  })

  it("flags a check that passes on bun but fails on node — the exact false-green this guards against", () => {
    // This is the scenario the shared applyExpectedFailures cannot see on its
    // own: a still-registered expected-failure entry for "bindings not
    // installed" would keep matching the Node failure by check::package::reason
    // alone, silently hiding that Bun has already started passing.
    const violations = detectRuntimeDivergence([
      {
        check: "import",
        package: "bindings",
        status: "pass",
        reason: "ok",
        evidence: "resolved",
        runtime: "bun",
      },
      {
        check: "import",
        package: "bindings",
        status: "fail",
        reason: "cannot import @alienplatform/bindings (package not installed)",
        evidence: "still throws",
        runtime: "node",
      },
    ])

    expect(violations).toHaveLength(1)
    expect(violations[0]).toMatchObject({ check: "runtime-divergence", package: "bindings" })
    expect(violations[0]?.reason).toBe("import passes on bun but fails on node")
    expect(violations[0]?.evidence).toBe("bun: resolved | node: still throws")
  })

  it("flags a check that passes on node but fails on bun (the other direction)", () => {
    const violations = detectRuntimeDivergence([
      {
        check: "import",
        package: "commands",
        status: "fail",
        reason: "boom",
        evidence: "bun fail",
        runtime: "bun",
      },
      {
        check: "import",
        package: "commands",
        status: "pass",
        reason: "ok",
        evidence: "node ok",
        runtime: "node",
      },
    ])

    expect(violations).toHaveLength(1)
    expect(violations[0]?.reason).toBe("import passes on node but fails on bun")
  })

  it("ignores results with no runtime tag (single-run checks like pack/install/typecheck)", () => {
    const violations = detectRuntimeDivergence([
      { check: "pack", package: "sdk", status: "pass", reason: "ok", evidence: "tarball" },
      {
        check: "install",
        package: "fixture",
        status: "fail",
        reason: "npm install failed",
        evidence: "exit 1",
      },
    ])

    expect(violations).toHaveLength(0)
  })

  it("does not false-positive when only one runtime ran a check (e.g. bun unavailable)", () => {
    const violations = detectRuntimeDivergence([
      {
        check: "import",
        package: "sdk",
        status: "fail",
        reason: "boom",
        evidence: "node only",
        runtime: "node",
      },
    ])

    expect(violations).toHaveLength(0)
  })

  it("keeps distinct check names for the same package independent (no cross-assertion bleed)", () => {
    // sdk reports three distinct import-family checks (import, import-error-
    // reexports, import-worker-runtime); a shared "import" name across all
    // three would let an unrelated pass/fail pairing masquerade as divergence.
    const violations = detectRuntimeDivergence([
      {
        check: "import",
        package: "sdk",
        status: "pass",
        reason: "ok",
        evidence: "a",
        runtime: "bun",
      },
      {
        check: "import",
        package: "sdk",
        status: "pass",
        reason: "ok",
        evidence: "b",
        runtime: "node",
      },
      {
        check: "import-worker-runtime",
        package: "sdk",
        status: "fail",
        reason: "subpath ./worker-runtime is not exported",
        evidence: "c",
        runtime: "bun",
      },
      {
        check: "import-worker-runtime",
        package: "sdk",
        status: "fail",
        reason: "subpath ./worker-runtime is not exported",
        evidence: "d",
        runtime: "node",
      },
    ])

    expect(violations).toHaveLength(0)
  })
})
