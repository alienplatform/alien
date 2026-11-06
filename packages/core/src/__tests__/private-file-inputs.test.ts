import { describe, expect, it } from "vitest"
import { getStackInputDefinitions, inputs, secret, string } from "../input.js"

const options = {
  providedBy: "deployer" as const,
  required: true,
  label: "Private configuration",
  description: "Written directly into the install input store",
}
const files = [{ path: "/var/run/alien/config/app.json", targetResources: ["api"], envName: "CONFIG_FILE" }]

describe("private file inputs", () => {
  it("keeps only the input reference and exact destinations in the authored stack", () => {
    const definitions = getStackInputDefinitions(inputs({ appConfig: secret({ ...options, files }) }))
    expect(definitions).toHaveLength(1)
    expect(definitions[0]).toMatchObject({ id: "appConfig", kind: "secret", providedBy: ["deployer"], files })
    expect(definitions[0]).not.toHaveProperty("value")
    expect(definitions[0]?.env).toEqual([])
  })

  it("rejects hosted or generated sources for private file bindings", () => {
    expect(() => inputs({ appConfig: secret({ ...options, providedBy: "developer", files }) })).toThrow("deployer-only")
    expect(() => inputs({ appConfig: secret({ ...options, providedBy: ["deployer", "developer"], files }) })).toThrow("deployer-only")
    expect(() => inputs({ appConfig: string({ ...options, files }) })).toThrow("deployer-only")
  })

  it("rejects traversal and incomplete file destinations", () => {
    for (const path of ["relative.json", "/var/run/../etc/config", "/var/run/./config", "/var/run/", "/var/run/\0config"]) {
      expect(() => inputs({ appConfig: secret({ ...options, files: [{ path }] }) })).toThrow("absolute file path")
    }
  })
})
