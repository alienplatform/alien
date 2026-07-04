import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { afterEach, beforeEach, describe, expect, it } from "vitest"

import {
  applyExpectedFailures,
  checkExportsTypes,
  checkForbiddenDeps,
  checkForbiddenSources,
  checkNoCommandsSubpath,
  checkSdkSubpathContainment,
  exitCodeFor,
} from "./validate-package-layout.js"

let tempDir: string

beforeEach(() => {
  tempDir = mkdtempSync(join(tmpdir(), "package-layout-test-"))
})

afterEach(() => {
  rmSync(tempDir, { recursive: true, force: true })
})

describe("checkForbiddenDeps", () => {
  it("flags a forbidden cloud SDK dependency in a bindings-like manifest", () => {
    const violations = checkForbiddenDeps(
      join(tempDir, "package.json"),
      { dependencies: { "@aws-sdk/client-s3": "^3.0.0" } },
      "bindings",
      "bindings",
    )

    expect(violations).toHaveLength(1)
    expect(violations[0]).toMatchObject({
      check: "forbidden-deps",
      package: "bindings",
    })
    expect(violations[0]?.reason).toMatch(/AWS SDK/)
  })

  it("passes a clean bindings-like manifest", () => {
    const violations = checkForbiddenDeps(
      join(tempDir, "package.json"),
      { dependencies: { "@alienplatform/core": "workspace:*" } },
      "bindings",
      "bindings",
    )

    expect(violations).toHaveLength(0)
  })
})

describe("checkForbiddenSources", () => {
  it("flags a nice-grpc import under a bindings-like source dir", () => {
    writeFileSync(
      join(tempDir, "storage.ts"),
      'import { createChannel } from "nice-grpc"\n\nexport const noop = () => createChannel\n',
    )

    const violations = checkForbiddenSources(tempDir, "bindings", "bindings")

    expect(violations.some(v => v.reason.includes("nice-grpc"))).toBe(true)
  })

  it("flags a reference to the forbidden ALIEN_BINDINGS_GRPC_ADDRESS env var", () => {
    writeFileSync(
      join(tempDir, "config.ts"),
      "export const address = process.env.ALIEN_BINDINGS_GRPC_ADDRESS\n",
    )

    const violations = checkForbiddenSources(tempDir, "bindings", "bindings")

    expect(violations.some(v => v.reason.includes("ALIEN_BINDINGS_GRPC_ADDRESS"))).toBe(true)
  })

  it("passes clean bindings sources", () => {
    writeFileSync(join(tempDir, "storage.ts"), "export const noop = () => 1\n")

    const violations = checkForbiddenSources(tempDir, "bindings", "bindings")

    expect(violations).toHaveLength(0)
  })

  it("flags a nice-grpc-common import with its own explicit reason", () => {
    writeFileSync(
      join(tempDir, "types.ts"),
      'import type { CallContext } from "nice-grpc-common"\n',
    )

    const violations = checkForbiddenSources(tempDir, "bindings", "bindings")

    expect(violations).toHaveLength(1)
    expect(violations[0]?.reason).toContain("nice-grpc-common")
  })

  it("flags an @alienplatform/sdk import (any subpath) under a commands-like source dir", () => {
    writeFileSync(
      join(tempDir, "receiver.ts"),
      'import { runWorker } from "@alienplatform/sdk/worker-runtime"\n',
    )

    const violations = checkForbiddenSources(tempDir, "commands", "commands")

    expect(violations.some(v => v.reason.includes("@alienplatform/sdk"))).toBe(true)
  })

  it("flags a generated Worker-protocol module import under a commands-like source dir", () => {
    writeFileSync(
      join(tempDir, "client.ts"),
      'import { ControlServiceDefinition } from "./generated/control.js"\n',
    )

    const violations = checkForbiddenSources(tempDir, "commands", "commands")

    expect(violations.some(v => v.reason.includes("Worker protocol"))).toBe(true)
  })

  it("passes clean commands sources (plain fetch, core imports)", () => {
    writeFileSync(
      join(tempDir, "client.ts"),
      'import { defineError } from "@alienplatform/core"\n\nexport const send = () => fetch("https://example.com")\n',
    )

    const violations = checkForbiddenSources(tempDir, "commands", "commands")

    expect(violations).toHaveLength(0)
  })

  it("returns no violations for a nonexistent source dir (ENOENT is not an error)", () => {
    const violations = checkForbiddenSources(
      join(tempDir, "does-not-exist"),
      "commands",
      "commands",
    )

    expect(violations).toHaveLength(0)
  })

  it("propagates non-ENOENT filesystem errors instead of swallowing them", () => {
    // Point the walker at a regular file: readdirSync raises ENOTDIR, which must
    // NOT be treated as "no sources" — only ENOENT may mean that.
    const filePath = join(tempDir, "not-a-dir.ts")
    writeFileSync(filePath, "export const x = 1\n")

    expect(() => checkForbiddenSources(filePath, "commands", "commands")).toThrow(/ENOTDIR/)
  })
})

describe("checkSdkSubpathContainment", () => {
  it("flags a gRPC import in packages/sdk/src OUTSIDE the worker-runtime dir", () => {
    writeFileSync(join(tempDir, "channel.ts"), 'import { createChannel } from "nice-grpc"\n')

    const violations = checkSdkSubpathContainment(tempDir, "sdk")

    expect(violations.some(v => v.reason.includes("nice-grpc"))).toBe(true)
  })

  it("passes the same gRPC import INSIDE the worker-runtime dir", () => {
    mkdirSync(join(tempDir, "worker-runtime"))
    writeFileSync(
      join(tempDir, "worker-runtime", "channel.ts"),
      'import { createChannel } from "nice-grpc"\n',
    )

    const violations = checkSdkSubpathContainment(tempDir, "sdk")

    expect(violations).toHaveLength(0)
  })

  it("flags a generated binding-service proto client anywhere in the package", () => {
    mkdirSync(join(tempDir, "generated"))
    writeFileSync(join(tempDir, "generated", "storage.ts"), "export const x = 1\n")

    const violations = checkSdkSubpathContainment(tempDir, "sdk")

    expect(violations.some(v => v.reason.includes("generated binding-service proto client"))).toBe(
      true,
    )
  })
})

describe("checkNoCommandsSubpath", () => {
  it("flags an exports map containing ./commands", () => {
    const violations = checkNoCommandsSubpath("package.json", { ".": {}, "./commands": {} }, "sdk")

    expect(violations).toHaveLength(1)
    expect(violations[0]).toMatchObject({ check: "no-commands-subpath", package: "sdk" })
  })

  it("passes an exports map without ./commands", () => {
    const violations = checkNoCommandsSubpath("package.json", { ".": {} }, "sdk")

    expect(violations).toHaveLength(0)
  })
})

describe("checkExportsTypes", () => {
  it("flags a subpath condition missing types", () => {
    const violations = checkExportsTypes(
      "package.json",
      { ".": { import: "./dist/index.js" } },
      "sdk",
    )

    expect(violations).toHaveLength(1)
    expect(violations[0]).toMatchObject({ check: "exports-types", package: "sdk" })
  })

  it("passes when every condition carries types", () => {
    const violations = checkExportsTypes(
      "package.json",
      { ".": { types: "./dist/index.d.ts", import: "./dist/index.js" } },
      "sdk",
    )

    expect(violations).toHaveLength(0)
  })
})

describe("applyExpectedFailures", () => {
  it("turns a listed failure into an expected (non-fatal) warning", () => {
    const result = applyExpectedFailures(
      [
        {
          check: "no-commands-subpath",
          package: "sdk",
          reason: "exports ./commands",
          evidence: "x",
        },
      ],
      [
        {
          check: "no-commands-subpath",
          package: "sdk",
          reason: "exports ./commands",
          owningTask: "03",
        },
      ],
    )

    expect(result.expected).toHaveLength(1)
    expect(result.fatal).toHaveLength(0)
    expect(result.stale).toHaveLength(0)
  })

  it("leaves an unlisted failure fatal", () => {
    const result = applyExpectedFailures(
      [{ check: "forbidden-deps", package: "bindings", reason: "AWS SDK", evidence: "x" }],
      [],
    )

    expect(result.fatal).toHaveLength(1)
    expect(result.expected).toHaveLength(0)
    expect(result.stale).toHaveLength(0)
  })

  it("reports an expected entry with no matching violation as stale (fatal to the run)", () => {
    const result = applyExpectedFailures(
      [],
      [
        {
          check: "no-commands-subpath",
          package: "sdk",
          reason: "exports ./commands",
          owningTask: "03",
        },
      ],
    )

    expect(result.stale).toHaveLength(1)
    expect(result.fatal).toHaveLength(0)
    expect(result.expected).toHaveLength(0)
  })
})

describe("exitCodeFor", () => {
  const violation = {
    check: "forbidden-deps",
    package: "bindings",
    reason: "AWS SDK",
    evidence: "x",
  }
  const staleEntry = {
    check: "no-commands-subpath",
    package: "sdk",
    reason: "exports ./commands",
    owningTask: "03",
  }

  it("returns 0 when there are no fatal violations and no stale expectations", () => {
    expect(exitCodeFor({ expected: [violation], fatal: [], stale: [] })).toBe(0)
  })

  it("returns 1 when any unexpected (fatal) violation exists", () => {
    expect(exitCodeFor({ expected: [], fatal: [violation], stale: [] })).toBe(1)
  })

  it("returns 1 when any stale expectation exists, even with zero violations", () => {
    expect(exitCodeFor({ expected: [], fatal: [], stale: [staleEntry] })).toBe(1)
  })
})
