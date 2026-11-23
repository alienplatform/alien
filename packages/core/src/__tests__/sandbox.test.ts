import { describe, expect, it } from "vitest"
import { Resource } from "../resource.js"
import { Sandbox, SandboxConfigSchema, type SandboxEgress } from "../sandbox.js"

const limits = { cpu: "1", memory: "2Gi", disk: "20Gi", maxProcesses: 256 }
const lifecycle = { maxLifetimeSeconds: 3600 }

function complete() {
  return new Sandbox("agent")
    .code({ type: "image", image: "ubuntu:24.04" })
    .limits(limits)
    .egress("none")
    .lifecycle(lifecycle)
}

describe("Sandbox", () => {
  it("builds a complete declaration", () => {
    const r = complete().build()

    expect(r.config.type).toBe("sandbox")
    expect(r.config.id).toBe("agent")
    expect(r.config.limits).toEqual(limits)
    expect(r.config.egress).toEqual("none")
  })

  // Ceilings are optional because not every platform can enforce them. Declaring none takes the
  // platform's own; declaring them where they cannot be enforced is what gets rejected, and that
  // rejection happens at plan time against the target platform rather than here.
  it("accepts a declaration that names no ceilings", () => {
    const sandbox = new Sandbox("agent")
      .code({ type: "image", image: "ubuntu:24.04" })
      .egress("none")
      .lifecycle(lifecycle)

    expect(sandbox.build().config.limits).toBeUndefined()
  })

  it("refuses a declaration missing its egress policy", () => {
    const sandbox = new Sandbox("agent")
      .code({ type: "image", image: "ubuntu:24.04" })
      .limits(limits)
      .lifecycle(lifecycle)

    expect(() => sandbox.build()).toThrow()
  })

  it("carries a runtime default independently of the declaration", () => {
    const r = complete()
      .egress("internet")
      .defaultPolicy({ allow: ["example.com"] })
      .build()

    expect(r.config.egress).toBe("internet")
    expect(r.config.defaultPolicy).toEqual({ allow: ["example.com"] })
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

  it("declares image permissions with a resource as a connection target", () => {
    const files = new Resource({ type: "acme/files", id: "workspace-files" })
    const r = complete()
      .egress("internet")
      .lifecycle({
        onPause: ["/usr/local/bin/app", "pause"],
        onResume: ["/usr/local/bin/app", "resume"],
      })
      .imagePermissions({
        mounts: [
          { type: "nfs", path: "/workspace" },
          { type: "tmpfs", path: "/workspace/tmp", size: "8Gi" },
        ],
        connections: [{ to: files, port: 2049 }],
      })
      .build()

    expect(r.config.imagePermissions).toEqual({
      mounts: [
        { type: "nfs", path: "/workspace" },
        { type: "tmpfs", path: "/workspace/tmp", size: "8Gi" },
      ],
      connections: [{ to: { type: "acme/files", id: "workspace-files" }, port: 2049 }],
    })
    expect(r.config.lifecycle).toEqual({
      onPause: ["/usr/local/bin/app", "pause"],
      onResume: ["/usr/local/bin/app", "resume"],
    })
  })

  it("refuses a mount type it does not know", () => {
    const sandbox = complete().imagePermissions({
      // @ts-expect-error: only nfs and tmpfs exist
      mounts: [{ type: "overlay", path: "/workspace" }],
    })
    expect(() => sandbox.build()).toThrow()
  })

  it("writes no imagePermissions when none are declared", () => {
    expect("imagePermissions" in complete().build().config).toBe(false)
  })

  it("links a service once, by resource or reference", () => {
    const api = new Resource({ type: "container", id: "api" })
    const r = complete()
      .egress("internet")
      .link(api)
      .link({ type: "container", id: "api" })
      .link({ type: "container", id: "billing" })
      .build()

    expect(r.config.gatewayLinks).toEqual([
      { type: "container", id: "api" },
      { type: "container", id: "billing" },
    ])
  })

  it("writes no gatewayLinks when none are declared", () => {
    expect("gatewayLinks" in complete().build().config).toBe(false)
  })

  it("exposes the resource type for permission targets", () => {
    expect(Sandbox.any()).toBe("sandbox")
  })
})

it("preserves the deprecated deny declaration and supervisor settings without gateway migration", () => {
  const policies: SandboxEgress[] = [{ mode: "deny" }]
  for (const egress of policies) {
    const config = SandboxConfigSchema.parse({
      ...complete().egress(egress).build().config,
      privilegedSupervisor: { commandUid: 60000 },
    })
    expect(config.egress).toEqual(egress)
    expect(config.privilegedSupervisor).toEqual({ commandUid: 60000 })
    expect(config.defaultPolicy).toBeUndefined()
  }
})

it("refuses the removed egress modes", () => {
  for (const egress of [{ mode: "allow" }, { mode: "allowDomains", domains: ["api.example.com"] }]) {
    expect(SandboxConfigSchema.safeParse({ ...complete().build().config, egress }).success).toBe(
      false,
    )
  }
})
