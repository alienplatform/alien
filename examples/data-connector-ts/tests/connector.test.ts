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

  // TypeScript binding access is unavailable between the binding-gRPC deletion
  // (ALIEN-217) and the direct-bindings addon (ALIEN-214/215) — unskip when
  // @alienplatform/bindings lands. This command hits the vault binding
  // unconditionally via getConnectionConfig().
  it.skip("should test connection using vault credentials", async () => {
    const result = await deployment.invokeCommand("test-connection", {})
    expect(result.connected).toBe(true)
    expect(result.database).toBe("warehouse")
    expect(result.host).toBe("db.customer.internal")
    // Password must never be exposed
    expect(result).not.toHaveProperty("password")
  })

  // TypeScript binding access is unavailable between the binding-gRPC deletion
  // (ALIEN-217) and the direct-bindings addon (ALIEN-214/215) — unskip when
  // @alienplatform/bindings lands. This command hits the vault binding
  // unconditionally via getConnectionConfig().
  it.skip("should query data", async () => {
    const result = await deployment.invokeCommand("query", {
      sql: "SELECT * FROM users",
    })
    expect(result.rows.length).toBeGreaterThan(0)
    expect(result.rows[0]).toHaveProperty("name")
    expect(result.cached).toBe(false)
  })

  // TypeScript binding access is unavailable between the binding-gRPC deletion
  // (ALIEN-217) and the direct-bindings addon (ALIEN-214/215) — unskip when
  // @alienplatform/bindings lands. This command hits the kv binding when
  // useCache is set.
  it.skip("should cache query results", async () => {
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
