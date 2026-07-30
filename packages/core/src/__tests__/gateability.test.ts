import { readFileSync } from "node:fs"
import { join } from "node:path"
import { describe, expect, it } from "vitest"
import * as alien from "../index.js"

/**
 * The SDK's `.enabled()` surface asserted against the generated gateability
 * manifest (`pnpm generate` rewrites it from the Rust policy), so the builder
 * surface and the policy cannot drift apart: a type the policy allows must
 * offer `.enabled()`, and a type it refuses must not.
 */
const manifest: Record<string, { frozen: boolean; live: boolean }> = JSON.parse(
  readFileSync(join(__dirname, "../generated/gateability.json"), "utf8"),
)

/**
 * Builder factories for every manifest type the TypeScript SDK exposes, each
 * carrying the minimum config its schema requires so `.build()` succeeds —
 * the gate is only proven by building the resource and reading the answer
 * back off it.
 */
const IMAGE = "registry.example.com/fixture:latest"

const builders: Record<string, () => object> = {
  kv: () => new alien.Kv("fixture"),
  storage: () => new alien.Storage("fixture"),
  queue: () => new alien.Queue("fixture"),
  vault: () => new alien.Vault("fixture"),
  postgres: () => new alien.Postgres("fixture"),
  ai: () => new alien.AI("fixture"),
  worker: () =>
    new alien.Worker("fixture").code({ type: "image", image: IMAGE }).permissions("execution"),
  daemon: () =>
    new alien.Daemon("fixture").code({ type: "image", image: IMAGE }).permissions("execution"),
  container: () =>
    new alien.Container("fixture")
      .code({ type: "image", image: IMAGE })
      .cpu(0.25)
      .memory("256Mi")
      .permissions("execution"),
  email: () => new alien.Email("fixture"),
  "experimental/aws-opensearch": () => new alien.experimental.AwsOpenSearch("fixture"),
}

describe("gateability manifest", () => {
  it("lists every type the builder table covers", () => {
    for (const resourceType of Object.keys(builders)) {
      expect(
        manifest,
        `builder table covers '${resourceType}' but the manifest does not`,
      ).toHaveProperty([resourceType])
    }
  })

  for (const [resourceType, gateability] of Object.entries(manifest)) {
    const gateable = gateability.frozen || gateability.live
    const makeBuilder = builders[resourceType]

    it(`'${resourceType}' ${gateable ? "offers" : "does not offer"} .enabled()`, () => {
      if (!makeBuilder) {
        // Types without a dedicated TypeScript builder (experimental or
        // Rust-only surface) cannot offer .enabled() anywhere, which is only
        // consistent when the policy refuses them too.
        expect(
          gateable,
          `the policy allows gating '${resourceType}' but the SDK has no builder exposing .enabled(); add the builder (and a matrix fixture) or refuse the type`,
        ).toBe(false)
        return
      }
      const builder = makeBuilder() as {
        enabled?: (input: { id: string }) => unknown
      }
      if (!gateable) {
        expect(
          builder.enabled,
          `the policy refuses gating '${resourceType}', so its builder must not expose .enabled()`,
        ).toBeUndefined()
        return
      }
      expect(
        typeof builder.enabled,
        `the policy allows gating '${resourceType}', so its builder must expose .enabled()`,
      ).toBe("function")

      // Exposing the method is not enough: a builder that accepts the input
      // and then drops it on the way into the Resource gates nothing, and
      // the resource would be created against the deployer's answer.
      const gated = (
        builder.enabled!({ id: "fixtureEnabled" }) as { build(): { enabledWhen?: string } }
      ).build()
      expect(
        gated.enabledWhen,
        `'${resourceType}' accepts .enabled() but does not carry the answer into its Resource`,
      ).toBe("fixtureEnabled")
    })
  }
})
