import { describe, expect, expectTypeOf, it } from "vitest"
import type { ExternalBinding } from "../generated/zod/external-binding-schema.js"
import { StackSettingsSchema } from "../generated/zod/stack-settings-schema.js"

const postgres = { host: "db.example.test", port: 5432, database: "demo", username: "demo" }
const bindings = [
  {
    type: "storage",
    service: "s3",
    bucketName: "demo",
    endpoint: "https://s3.example.test",
    region: "us-east-1",
    forcePathStyle: true,
  },
  { type: "storage", service: "blob", accountName: "demo", containerName: "archive" },
  { type: "storage", service: "gcs", bucketName: "demo" },
  { type: "storage", service: "local-storage", storagePath: "/tmp/demo" },
  { type: "queue", service: "sqs", queueUrl: "https://queue.example.test/demo" },
  { type: "queue", service: "pubsub", topic: "demo", subscription: "reader" },
  { type: "queue", service: "servicebus", namespace: "demo", queueName: "events" },
  { type: "queue", service: "local-queue", queuePath: "/tmp/demo" },
  {
    type: "kv",
    service: "dynamodb",
    tableName: "demo",
    region: "us-east-1",
    endpointUrl: "https://kv.example.test",
  },
  {
    type: "kv",
    service: "firestore",
    projectId: "demo",
    databaseId: "demo",
    collectionName: "items",
  },
  {
    type: "kv",
    service: "tablestorage",
    resourceGroupName: "demo",
    accountName: "demo",
    tableName: "items",
  },
  {
    type: "kv",
    service: "redis",
    connectionUrl: "redis://localhost:6379",
    keyPrefix: "demo",
    database: 2,
  },
  { type: "kv", service: "local-kv", dataDir: "/tmp/demo", keyPrefix: "demo" },
  {
    type: "artifact_registry",
    service: "ecr",
    repositoryPrefix: "demo",
    pullRoleArn: "demo-pull",
    pushRoleArn: "demo-push",
  },
  {
    type: "artifact_registry",
    service: "acr",
    registryName: "demo",
    resourceGroupName: "demo",
    repositoryPrefix: "demo",
  },
  {
    type: "artifact_registry",
    service: "gar",
    repositoryName: "demo",
    pullServiceAccountEmail: "pull@example.test",
    pushServiceAccountEmail: "push@example.test",
  },
  { type: "artifact_registry", service: "local", registryUrl: "localhost:5000", dataDir: null },
  { type: "vault", service: "parameter-store", vaultPrefix: "demo" },
  { type: "vault", service: "secret-manager", vaultPrefix: "demo" },
  { type: "vault", service: "key-vault", vaultName: "demo" },
  { type: "vault", service: "kubernetes-secret", namespace: "demo", vaultPrefix: "demo" },
  { type: "vault", service: "local-vault", vaultName: "demo", dataDir: "/tmp/demo" },
  {
    type: "container_apps_environment",
    environmentName: "demo",
    resourceId: "demo",
    resourceGroupName: "demo",
    defaultDomain: "example.test",
    staticIp: "192.0.2.1",
  },
  {
    type: "postgres",
    service: "aurora",
    clusterEndpoint: "db.example.test",
    port: 5432,
    database: "demo",
    username: "demo",
    passwordSecretArn: "demo-secret",
  },
  {
    type: "postgres",
    service: "cloud-sql",
    ...postgres,
    serverCaCertificates: ["demo-ca"],
    passwordSecretName: "demo-secret",
  },
  {
    type: "postgres",
    service: "flexible-server",
    ...postgres,
    passwordSecretUri: "https://vault.example.test/demo",
  },
  {
    type: "postgres",
    service: "external",
    ...postgres,
    password: "demo-only",
    sslMode: "verify-full",
  },
  { type: "postgres", service: "local-postgres", ...postgres, password: "demo-only" },
  { type: "ai", provider: "openai", apiKey: { secretRef: { name: "demo", key: "api-key" } } },
] satisfies ExternalBinding[]

describe("StackSettings external bindings", () => {
  it("preserves distinct arbitrary resource IDs and their typed values", () => {
    const externalBindings = {
      "archive-primary": { type: "storage", service: "s3", bucketName: "demo-primary" },
      "archive.secondary": { type: "storage", service: "s3", bucketName: "demo-secondary" },
    }
    const parsed = StackSettingsSchema.parse({ externalBindings, endpointAccess: "private" })
    expect(parsed.externalBindings).toEqual(externalBindings)
    expect(parsed.endpointAccess).toBe("private")
  })

  it.each(bindings)("preserves every field of $type / $service", binding => {
    const externalBindings = { "arbitrary/resource": binding }
    const parsed = StackSettingsSchema.parse({ externalBindings })
    expect(parsed.externalBindings).toEqual(externalBindings)
    expectTypeOf(parsed.externalBindings)
      .exclude<null | undefined>()
      .toEqualTypeOf<Record<string, ExternalBinding>>()
  })

  it.each([
    "demo-bucket",
    { secretRef: { name: "demo", key: "bucket" } },
    { "Fn::Join": ["-", [{ Ref: "BucketPrefix" }, "archive"]] },
    { nested: { list: [null, true, 42, "demo"] } },
  ])("preserves globally supported BindingValue forms: %j", bucketName => {
    const externalBindings = { archive: { type: "storage", service: "s3", bucketName } }
    expect(StackSettingsSchema.parse({ externalBindings }).externalBindings).toEqual(
      externalBindings,
    )
  })

  it.each([
    { type: "unknown", service: "s3", bucketName: "demo" },
    { type: "storage", service: "unknown", bucketName: "demo" },
    { type: "storage", service: "s3" },
    { type: "storage", service: "s3", bucketName: "demo", forcePathStyle: "true" },
    { type: "ai", provider: 42, apiKey: "demo-only" },
    { type: "postgres", service: "external", ...postgres, password: 42 },
    {
      type: "postgres",
      service: "external",
      ...postgres,
      password: "demo-only",
      sslMode: "unknown",
    },
    { type: "storage", service: "s3", bucketName: () => "not-json" },
  ])("rejects invalid binding values: %j", binding => {
    expect(StackSettingsSchema.safeParse({ externalBindings: { archive: binding } }).success).toBe(
      false,
    )
  })
})
