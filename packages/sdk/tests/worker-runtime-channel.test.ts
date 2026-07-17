import { afterEach, describe, expect, it, vi } from "vitest"
import { getGrpcEndpoint, getGrpcEndpointConfig } from "../src/worker-runtime/channel.js"
import {
  getControlServiceDefinition,
  getWaitUntilServiceDefinition,
} from "../src/worker-runtime/service-definitions.js"

const WORKER_GRPC_ADDRESS = "ALIEN_WORKER_GRPC_ADDRESS"
const LEGACY_GRPC_ADDRESS = "ALIEN_BINDINGS_GRPC_ADDRESS"

describe("getGrpcEndpoint", () => {
  afterEach(() => {
    vi.unstubAllEnvs()
  })

  it("uses the Worker protocol address when both runtime generations provide an address", () => {
    vi.stubEnv(WORKER_GRPC_ADDRESS, "127.0.0.1:60000")
    vi.stubEnv(LEGACY_GRPC_ADDRESS, "127.0.0.1:50000")

    expect(getGrpcEndpoint()).toBe("127.0.0.1:60000")
    expect(getGrpcEndpointConfig().generation).toBe("current")
    expect(getControlServiceDefinition().fullName).toBe("alien_worker.control.ControlService")
    expect(getWaitUntilServiceDefinition().fullName).toBe(
      "alien_worker.wait_until.WaitUntilService",
    )
  })

  it("accepts the address inherited from a runtime released before the protocol rename", () => {
    vi.stubEnv(WORKER_GRPC_ADDRESS, undefined)
    vi.stubEnv(LEGACY_GRPC_ADDRESS, "127.0.0.1:51351")

    expect(getGrpcEndpoint()).toBe("127.0.0.1:51351")
    expect(getGrpcEndpointConfig().generation).toBe("legacy")
    expect(getControlServiceDefinition().fullName).toBe("alien_bindings.control.ControlService")
    expect(getWaitUntilServiceDefinition().fullName).toBe(
      "alien_bindings.wait_until.WaitUntilService",
    )
  })

  it("still fails with the current variable name when no runtime address is present", () => {
    vi.stubEnv(WORKER_GRPC_ADDRESS, undefined)
    vi.stubEnv(LEGACY_GRPC_ADDRESS, undefined)

    expect(() => getGrpcEndpoint()).toThrow(
      "Required environment variable 'ALIEN_WORKER_GRPC_ADDRESS' is not set",
    )
  })
})
