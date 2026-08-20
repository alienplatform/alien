import { describe, expect, it } from "vitest"
import { Sandbox } from "../sandbox.js"

const limits = { cpu: "1", memory: "2Gi", disk: "20Gi", maxProcesses: 256 }
const session = { maxLifetimeSeconds: 3600 }

function complete() {
  return new Sandbox("agent")
    .code({ type: "image", image: "ubuntu:24.04" })
    .limits(limits)
    .egress({ mode: "deny" })
    .session(session)
}

describe("Sandbox", () => {
  it("builds a complete declaration", () => {
    const r = complete().build()

    expect(r.config.type).toBe("sandbox")
    expect(r.config.id).toBe("agent")
    expect(r.config.limits).toEqual(limits)
    expect(r.config.egress).toEqual({ mode: "deny" })
  })

  // Ceilings are optional because not every platform can enforce them. Declaring none takes the
  // platform's own; declaring them where they cannot be enforced is what gets rejected, and that
  // rejection happens at plan time against the target platform rather than here.
  it("accepts a declaration that names no ceilings", () => {
    const sandbox = new Sandbox("agent")
      .code({ type: "image", image: "ubuntu:24.04" })
      .egress({ mode: "deny" })
      .session(session)

    expect(sandbox.build().config.limits).toBeUndefined()
  })

  it("refuses a declaration missing its egress policy", () => {
    const sandbox = new Sandbox("agent")
      .code({ type: "image", image: "ubuntu:24.04" })
      .limits(limits)
      .session(session)

    expect(() => sandbox.build()).toThrow()
  })

  it("carries a hostname allowlist through the schema", () => {
    const r = complete()
      .egress({ mode: "allowDomains", domains: ["example.com"] })
      .build()

    expect(r.config.egress).toEqual({ mode: "allowDomains", domains: ["example.com"] })
  })

  it("carries declared preview ports", () => {
    const r = complete().previewPorts([8080, 9090]).build()
    expect(r.config.previewPorts).toEqual([8080, 9090])
  })

  // The schema refuses unknown fields, so writing to the builder's private state cannot smuggle
  // one into the built config.
  it("strips an unknown field at build time", () => {
    const sandbox = complete()
    ;(sandbox as unknown as { _config: Record<string, unknown> })._config.anonymous = true

    const r = sandbox.build()
    expect(r.config).not.toHaveProperty("anonymous")
  })

  it("exposes the resource type for permission targets", () => {
    expect(Sandbox.any()).toBe("sandbox")
  })
})
