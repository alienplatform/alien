import { describe, expect, it } from "vitest"
import * as alien from "../index.js"

describe("stack operations", () => {
  const inputs = alien.inputs({
    dbUrl: alien.string({
      label: "Database URL",
      description: "ClickHouse URL",
      providedBy: "deployer",
      required: true,
    }),
    dbPassword: alien.secret({
      label: "Database password",
      description: "ClickHouse password",
      providedBy: "deployer",
      required: true,
    }),
  })
  const data = new alien.Storage("data").build()

  it("stores built-in plugins, settings, approval and pinned custom plugins", () => {
    const stack = new alien.Stack("app")
      .inputs(inputs)
      .add(data, "frozen")
      .operations({
        kubernetes: { approval: { "*": "auto" } },
        clickhouse: { url: inputs.dbUrl, password: inputs.dbPassword, database: "app" },
        s3: { buckets: [data] },
        plugins: ["mine@1.2.0", { name: "other", version: "0.1.0", approval: { "*": "manual" } }],
      })
      .build()

    expect(stack.operations).toEqual({
      plugins: {
        kubernetes: { approval: { "*": "auto" } },
        clickhouse: {
          settings: { url: { input: "dbUrl" }, password: { input: "dbPassword" }, database: "app" },
        },
        s3: { settings: { buckets: { resources: ["data"] } } },
      },
      custom: [
        { name: "mine", version: "1.2.0" },
        { name: "other", version: "0.1.0", approval: { "*": "manual" } },
      ],
    })
  })

  it("rejects a custom plugin without an exact version and an empty declaration", () => {
    expect(() => new alien.Stack("app").operations({ plugins: ["mine"] })).toThrow("name@version")
    expect(() => new alien.Stack("app").operations({})).toThrow("at least one plugin")
  })
})
