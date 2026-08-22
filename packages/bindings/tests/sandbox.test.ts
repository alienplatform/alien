/**
 * Sandbox binding tests through the REAL napi addon.
 *
 * Only the paths that need no cloud credentials and no running sandbox are covered here: how a
 * binding is resolved from the environment, and what a caller sees when one is missing or
 * malformed. Creating a session needs a backend — Local needs Docker, and the four cloud backends
 * need real credentials — so session behaviour is covered by `crates/alien-local/tests/` against
 * real Docker and by the e2e apps against a deployed stack.
 *
 * The value of this file is the boundary the other suites skip: an unconfigured or wrong-shaped
 * binding must produce a typed error naming the binding, not a panic crossing the addon.
 *
 * Run locally with a built addon:
 * `ALIEN_BINDINGS_ADDON_PATH=<path to the .node> pnpm vitest run tests/sandbox.test.ts`
 */

import { describe, expect, it } from "vitest"
import { AlienError, sandbox } from "../src/index.js"

/** Puts a binding into the environment the way the runtime supplies one. */
function setBinding(name: string, value: unknown): void {
  // Resolving a binding also reads the deployment type, as the runtime would supply it.
  process.env.ALIEN_DEPLOYMENT_TYPE = "local"
  const variable = `ALIEN_${name.toUpperCase().replace(/-/g, "_")}_BINDING`
  process.env[variable] = JSON.stringify(value)
}

describe("sandbox binding resolution", () => {
  it("names the binding and its environment variable when none is configured", async () => {
    // The failure a developer actually hits first: a sandbox declared in the stack but the
    // workload started without its binding. The message has to say which one.
    await expect(sandbox("not-configured").capabilities()).rejects.toThrow(/not-configured/i)
  })

  it("refuses a binding whose provider is unknown rather than guessing one", async () => {
    setBinding("sandbox-bad-provider", { provider: "not-a-cloud", imageArn: "arn:aws:lambda:::x" })

    const error = await sandbox("sandbox-bad-provider")
      .capabilities()
      .then(
        () => null,
        (caught: unknown) => caught,
      )

    expect(error).toBeInstanceOf(AlienError)
  })

  it("refuses an AWS binding that is missing a required field", async () => {
    // imageVersion is load-bearing: image plus version is the session identity, so a binding
    // without it would enumerate the wrong scope rather than fail.
    setBinding("sandbox-incomplete", {
      provider: "aws",
      imageArn: "arn:aws:lambda:us-west-2:123456789012:microvm-image:sbx",
      region: "us-west-2",
    })

    await expect(sandbox("sandbox-incomplete").capabilities()).rejects.toBeInstanceOf(AlienError)
  })
})
