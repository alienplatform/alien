import { describe, expect, it } from "vitest"
import { Redis } from "../redis.js"

describe("Redis", () => {
  it("defaults to volatile-lru with persistence on and no HA", () => {
    const r = new Redis("cache").build()
    expect(r.config).toMatchObject({
      type: "redis",
      id: "cache",
      evictionPolicy: "volatile-lru",
      persistence: true,
      highAvailability: false,
    })
    expect(r.config).not.toHaveProperty("memory")
  })

  it("carries every option through to the resource config", () => {
    const r = new Redis("queue")
      .memory("4Gi")
      .evictionPolicy("noeviction")
      .persistence(false)
      .highAvailability()
      .build()
    expect(r.config).toMatchObject({
      memory: "4Gi",
      evictionPolicy: "noeviction",
      persistence: false,
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
