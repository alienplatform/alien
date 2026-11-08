import { describe, expect, it } from "vitest"
import { Redis } from "../redis.js"

describe("Redis", () => {
  it("defaults to volatile-lru with persistence off and no HA", () => {
    const r = new Redis("cache").build()
    expect(r.config).toMatchObject({
      type: "redis",
      id: "cache",
      evictionPolicy: "volatile-lru",
      persistence: false,
      highAvailability: false,
    })
    expect(r.config).not.toHaveProperty("memory")
  })

  it("carries every option through to the resource config", () => {
    const r = new Redis("queue")
      .memory("4Gi")
      .evictionPolicy("noeviction")
      .persistence()
      .highAvailability()
      .build()
    expect(r.config).toMatchObject({
      memory: "4Gi",
      evictionPolicy: "noeviction",
      persistence: true,
      highAvailability: true,
    })
  })

  it("rejects an eviction policy the server does not know", () => {
    const redis = new Redis("cache").evictionPolicy("allkeys-lru")
    ;(redis as unknown as { _config: Record<string, unknown> })._config.evictionPolicy = "lru"
    expect(() => redis.build()).toThrow()
  })

  it("is addressable as a permission target type", () => {
    expect(Redis.any()).toBe("redis")
  })
})
