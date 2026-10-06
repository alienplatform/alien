import { describe, expect, expectTypeOf, it } from "vitest"
import { AlienCore } from "../../../../client-sdks/platform/typescript/src/core.js"
import { deploymentPrepareStack } from "../../../../client-sdks/platform/typescript/src/funcs/deploymentPrepareStack.js"
import { deploymentsUpdateInputs } from "../../../../client-sdks/platform/typescript/src/funcs/deploymentsUpdateInputs.js"
import { HTTPClient } from "../../../../client-sdks/platform/typescript/src/lib/http.js"
import { SDKValidationError } from "../../../../client-sdks/platform/typescript/src/models/errors/sdkvalidationerror.js"
import type { ExternalBindingUnion } from "../../../../client-sdks/platform/typescript/src/models/index.js"
import type { PrepareDeploymentStackRequest } from "../../../../client-sdks/platform/typescript/src/models/operations/index.js"
import { ExternalBindingSchema as GeneratedExternalBindingSchema } from "../generated/zod/external-binding-schema.js"
import { ExternalBindingsSchema as GeneratedExternalBindingsSchema } from "../generated/zod/external-bindings-schema.js"
import { ExternalBindingSchema, ExternalBindingsSchema, ValueSchema } from "../index.js"

function captureClient() {
  const requests: Array<{ method: string; path: string; body: unknown }> = []
  const client = new AlienCore({
    serverURL: "https://api.example.invalid",
    httpClient: new HTTPClient({
      fetcher: async (input, init) => {
        const request = new Request(input, init)
        requests.push({
          method: request.method,
          path: new URL(request.url).pathname,
          body: JSON.parse(await request.text()),
        })
        // A deterministic API refusal lets this test focus on the request boundary.
        return Response.json(
          { code: "TEST_REFUSAL", message: "Fixture response", internal: false },
          { status: 400 },
        )
      },
    }),
  })
  return { client, requests }
}

function preparation(externalBindings: unknown): PrepareDeploymentStackRequest {
  // Invalid-input cases deliberately enter through the public runtime boundary.
  return {
    platform: "aws",
    setupMethod: "cli",
    saveForSetup: true,
    deploymentId: "dep_demo",
    updateOperationId: "op_demo",
    stackSettings: { externalBindings, endpointAccess: "private" },
  } as PrepareDeploymentStackRequest
}

const archive = {
  type: "storage",
  service: "s3",
  bucketName: "demo-archive",
  region: "us-east-1",
  forcePathStyle: true,
}

describe("deployment request serialization", () => {
  it("types arbitrary binding keys with the generated binding union", () => {
    type Bindings = NonNullable<
      NonNullable<PrepareDeploymentStackRequest["stackSettings"]>["externalBindings"]
    >
    expectTypeOf<Bindings>().toEqualTypeOf<Record<string, ExternalBindingUnion>>()
    expectTypeOf<Bindings[string]>().not.toBeAny()
  })

  it("exports the original binding validators for shared schema registration", () => {
    expect(ExternalBindingSchema).toBe(GeneratedExternalBindingSchema)
    expect(ExternalBindingsSchema).toBe(GeneratedExternalBindingsSchema)
    expect(ExternalBindingsSchema.parse({ archive })).toEqual({ archive })
    expect(
      ExternalBindingsSchema.safeParse({ archive: { ...archive, forcePathStyle: "true" } }).success,
    ).toBe(false)
  })

  it.each(["aws", "machines"] as const)(
    "sends arbitrary binding keys and exact %s setup target IDs over HTTP",
    async platform => {
      const { client, requests } = captureClient()
      const externalBindings = {
        archive,
        events: { type: "queue", service: "sqs", queueUrl: "https://sqs.example.invalid/demo" },
      }
      const request = { ...preparation(externalBindings), platform }
      await deploymentPrepareStack(client, request)
      expect(requests).toEqual([
        {
          method: "POST",
          path: "/v1/deployment-info/prepare-stack",
          body: request,
        },
      ])
    },
  )

  it.each([
    ["non-map", []],
    ["unknown discriminator", { archive: { ...archive, type: "unknown" } }],
    ["unknown service", { archive: { ...archive, service: "unknown" } }],
    ["invalid nested boolean", { archive: { ...archive, forcePathStyle: "true" } }],
  ])("rejects %s before HTTP", async (_name, bindings) => {
    const { client, requests } = captureClient()
    const result = await deploymentPrepareStack(client, preparation(bindings))
    expect(requests).toEqual([])
    expect(result.ok).toBe(false)
    if (result.ok) throw new Error("Invalid input unexpectedly succeeded")
    expect(result.error).toBeInstanceOf(SDKValidationError)
  })

  it.each([
    archive,
    { type: "queue", service: "sqs", queueUrl: "https://queue.example.invalid/demo" },
    { type: "kv", service: "redis", connectionUrl: "redis://localhost:6379", database: 2 },
    {
      type: "artifact_registry",
      service: "ecr",
      repositoryPrefix: "demo",
      pullRoleArn: "demo-pull",
      pushRoleArn: "demo-push",
    },
    { type: "vault", service: "parameter-store", vaultPrefix: "demo" },
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
      service: "external",
      host: "db.example.test",
      port: 5432,
      database: "demo",
      username: "demo",
      password: "demo-only",
      sslMode: "verify-full",
    },
    { type: "ai", provider: "openai", apiKey: { secretRef: { name: "demo", key: "api-key" } } },
  ])("preserves the $type binding variant over HTTP", async binding => {
    const { client, requests } = captureClient()
    const request = preparation({ "arbitrary/resource": binding })
    await deploymentPrepareStack(client, request)
    expect(requests).toHaveLength(1)
    expect(requests[0]?.body).toEqual(request)
  })

  it.each([
    null,
    true,
    42,
    [null, false, "demo"],
    "demo-bucket",
    { secretRef: { name: "demo", key: "bucket" } },
    { "Fn::Join": ["-", [{ Ref: "BucketPrefix" }, "archive"]] },
    { nested: { list: [null, true, 42, "demo"] } },
  ])("preserves JSON binding inputs over HTTP: %j", async bucketName => {
    const { client, requests } = captureClient()
    const request = preparation({
      archive: { ...archive, bucketName: ValueSchema.parse(bucketName) },
    })
    await deploymentPrepareStack(client, request)
    expect(requests).toHaveLength(1)
    expect(requests[0]?.body).toEqual(request)
  })

  it.each([
    undefined,
    () => "demo",
    Symbol("demo"),
    1n,
    NaN,
    Infinity,
    { nested: undefined },
    [() => "demo"],
  ])("keeps the canonical runtime JSON validator strict (case %#)", value => {
    expect(ValueSchema.safeParse(value).success).toBe(false)
  })

  it("sends input updates with the expected base operation CAS", async () => {
    const { client, requests } = captureClient()
    const body = { inputValues: { enabled: true }, expectedBaseOperationId: "op_demo" }
    await deploymentsUpdateInputs(client, { id: "dep_demo", updateDeploymentInputsRequest: body })
    expect(requests).toEqual([
      {
        method: "PATCH",
        path: "/v1/deployments/dep_demo/inputs",
        body,
      },
    ])
  })
})
