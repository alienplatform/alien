import { type Deployment, deploy } from "@alienplatform/testing"
import { afterAll, beforeAll, describe, expect, it } from "vitest"

describe("data-connector-ts", () => {
  let deployment: Deployment

  beforeAll(async () => {
    deployment = await deploy({ app: ".", platform: "local" })

    // Set up database credentials in the customer's vault.
    // In production, the customer stores these in their native secret manager
    // (AWS Secrets Manager, GCP Secret Manager, or Azure Key Vault).
    // The vendor never sees the password.
    await deployment.setExternalSecret(
      "credentials",
      "database",
      JSON.stringify({
        host: "db.customer.internal",
        port: 5432,
        database: "warehouse",
        user: "readonly",
        password: "customer-secret-password",
      }),
    )
  }, 300_000)

  afterAll(async () => {
    await deployment?.destroy()
  })

  it("should test connection using vault credentials", async () => {
    const result = await deployment.invokeCommand("test-connection", {})
    expect(result.connected).toBe(true)
    expect(result.database).toBe("warehouse")
    expect(result.host).toBe("db.customer.internal")
    // Password must never be exposed
    expect(result).not.toHaveProperty("password")
  })

  it("should query data", async () => {
    const result = await deployment.invokeCommand("query", {
      sql: "SELECT * FROM users",
    })
    expect(result.rows.length).toBeGreaterThan(0)
    expect(result.rows[0]).toHaveProperty("name")
    expect(result.cached).toBe(false)
  })

  it("should cache query results", async () => {
    // First query populates cache
    await deployment.invokeCommand("query", {
      sql: "SELECT * FROM orders",
      useCache: true,
    })

    // Second query should hit cache
    const result = await deployment.invokeCommand("query", {
      sql: "SELECT * FROM orders",
      useCache: true,
    })
    expect(result.cached).toBe(true)
    expect(result.rows.length).toBeGreaterThan(0)
  })

  it("should list available tables", async () => {
    const result = await deployment.invokeCommand("list-tables", {})
    expect(result.tables).toContain("users")
    expect(result.tables).toContain("orders")
  })
})
