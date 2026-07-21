import { randomUUID } from "node:crypto"
import { afterAll, describe, expect, it } from "vitest"
import { container } from "../src/index.js"
import { cleanupTempDirs, localContainerBindingEnv } from "./helpers/local-binding-env.js"

describe("linked container", () => {
  afterAll(cleanupTempDirs)

  it("returns internal and optional public URLs through the real addon", async () => {
    const isBun = process.env.BUN_EXPECTED === "1"
    const name = isBun ? "bun-container" : `container-${randomUUID()}`
    if (!isBun) localContainerBindingEnv(name)
    const database = container(name)

    await expect(database.getInternalUrl()).resolves.toBe("http://database.internal:5432")
    await expect(database.getPublicUrl()).resolves.toBe("http://localhost:15432")
  })
})
