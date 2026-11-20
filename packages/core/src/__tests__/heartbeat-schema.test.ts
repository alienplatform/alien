import { describe, expect, it } from "vitest"
import { z } from "zod/v4"
import { CustomResourceTypeSchema, ResourceHeartbeatDataSchema } from "../generated/index.js"

const status = {
  health: "healthy",
  lifecycle: "unknown",
  message: null,
  stale: false,
  partial: false,
  collectionIssues: [],
}

describe("ResourceHeartbeatDataSchema", () => {
  it("loads an extension's heartbeat with its details", () => {
    const heartbeat = {
      resourceType: "acme/dns-zone",
      data: { status, details: { records: 12 } },
    }
    expect(ResourceHeartbeatDataSchema.parse(heartbeat)).toEqual(heartbeat)
  })

  // A built-in heartbeat must fail the custom variant rather than load as custom with its own
  // fields stripped, whichever variant a validator tries first.
  it("keeps every field of a built-in heartbeat", () => {
    const heartbeat = {
      resourceType: "service-account",
      data: { backend: "local", status, identity: "sa", configured: true },
    }
    expect(ResourceHeartbeatDataSchema.parse(heartbeat)).toEqual(heartbeat)
  })

  // A custom tag is typed `${string}/${string}`, so comparing against a built-in tag still narrows
  // to that variant; this fails to compile if the custom tag widens to `string`.
  it("narrows to a built-in variant by its tag", () => {
    const heartbeat = ResourceHeartbeatDataSchema.parse({
      resourceType: "service-account",
      data: { backend: "local", status, identity: "sa", configured: true },
    })
    if (heartbeat.resourceType !== "service-account") {
      throw new Error(`expected a service-account heartbeat, got ${heartbeat.resourceType}`)
    }
    expect(heartbeat.data.backend).toBe("local")
  })

  it.each(["not-a-resource", "alien/dns-zone", "experimental/dns-zone"])(
    "refuses %s, which is neither built in nor an extension's type",
    resourceType => {
      expect(
        ResourceHeartbeatDataSchema.safeParse({ resourceType, data: { status } }).success,
      ).toBe(false)
    },
  )

  // One field, one schema: the OpenAPI specs built from these schemas feed the Rust SDK
  // generator, which can't merge two patterned strings, so the type must stay a single string.
  it("describes an extension's type as one string with alien-core's pattern", () => {
    expect(z.toJSONSchema(CustomResourceTypeSchema)).toMatchObject({
      type: "string",
      pattern: "^(?!(?:alien|experimental)\\/)[^/]+\\/.+$",
    })
    expect(z.toJSONSchema(CustomResourceTypeSchema)).not.toHaveProperty("allOf")
  })

  // The custom variant stays last, as alien-core lists it: the specs built from this union feed
  // the Rust SDK generator, whose union tries members in order and lets an earlier one take a
  // later one's fields.
  it("keeps the custom variant after every built-in one", () => {
    const variants = ResourceHeartbeatDataSchema.options
    const custom = { resourceType: "acme/dns-zone", data: { status } }
    expect(variants.at(-1)?.safeParse(custom).success).toBe(true)
    expect(variants.slice(0, -1).some(variant => variant.safeParse(custom).success)).toBe(false)
  })
})
