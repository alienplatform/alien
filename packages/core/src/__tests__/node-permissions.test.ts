import { describe, expect, it } from "vitest"
import { ComputeCluster, Daemon, type PermissionProfile, Storage } from "../index.js"

describe("explicit node and workload permissions", () => {
  it("keeps inline node grants separate from workload profiles", () => {
    const permissions: PermissionProfile = { objects: ["storage/data-read"] }
    const cluster = new ComputeCluster("compute").nodePermissions(permissions).build()
    expect(cluster.config).toMatchObject({ nodePermissions: permissions })
    expect(new ComputeCluster("compute").build().config).not.toHaveProperty("nodePermissions")
    expect(cluster.config).not.toHaveProperty("permissions")
  })

  it("serializes an optional node platform selector without changing cluster identity or profile", () => {
    const permissions: PermissionProfile = { objects: ["storage/data-read"] }
    const cluster = new ComputeCluster("compute").nodePermissions(permissions, {
      platforms: ["aws"],
    })
    const selected = JSON.parse(JSON.stringify(cluster.build().config))
    expect(selected.id).toBe("compute")
    expect(selected.nodePermissions).toEqual(permissions)
    expect(selected.nodePermissionsPlatforms).toEqual(["aws"])
    const unselected = JSON.parse(
      JSON.stringify(cluster.nodePermissions(permissions).build().config),
    )
    expect(unselected.id).toBe("compute")
    expect(unselected.nodePermissions).toEqual(permissions)
    expect(unselected).not.toHaveProperty("nodePermissionsPlatforms")
  })

  it("preserves daemon links without inventing a permission profile", () => {
    const storage = new Storage("objects").build()
    const daemon = new Daemon("observer")
      .code({ type: "image", image: "observer:latest" })
      .link(storage)
    const config = daemon.build().config
    expect(config).toMatchObject({ links: [{ type: "storage", id: "objects" }] })
    expect(config).not.toHaveProperty("permissions")
    expect(daemon.permissions("reader").build().config).toMatchObject({ permissions: "reader" })
  })
})
