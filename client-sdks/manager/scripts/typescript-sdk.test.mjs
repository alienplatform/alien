import assert from "node:assert/strict"
import test from "node:test"
import { HTTPClient } from "../typescript/esm/lib/http.js"
import { AlienManager } from "../typescript/esm/sdk/sdk.js"

import { computeSettingsFromJSON, computeSettingsToJSON } from "../typescript/esm/models/computesettings.js"

import { agentSyncRequestToJSON } from "../typescript/esm/models/agentsyncrequest.js"
import { createCommandResponseFromJSON } from "../typescript/esm/models/createcommandresponse.js"
import { healthResponseFromJSON } from "../typescript/esm/models/healthresponse.js"
import { stackImportRequestToJSON } from "../typescript/esm/models/stackimportrequest.js"
import { stackSettingsFromJSON } from "../typescript/esm/models/stacksettings.js"

test("setup imports retain explicit authority without elevating omitted settings", () => {
  for (const setupManagement of [undefined, "manual", "automatic"]) {
    const stackSettings = setupManagement === undefined ? {} : { setupManagement }
    const request = JSON.parse(stackImportRequestToJSON({
      deploymentGroupToken: "synthetic-token",
      deploymentName: "example",
      platform: "aws",
      region: "us-east-1",
      resourcePrefix: "example",
      resources: [],
      setupFingerprint: "synthetic-fingerprint",
      setupFingerprintVersion: 1,
      setupImportFormatVersion: 1,
      setupTarget: "aws",
      stackSettings,
    }))
    assert.deepEqual(request.stackSettings, stackSettings)
    const response = stackSettingsFromJSON(JSON.stringify(stackSettings))
    assert.equal(response.ok, true)
    if (response.ok) assert.deepEqual(response.value, stackSettings)
  }
})

for (const scenario of [
  { method: "setDeploymentChannel", suffix: "channel", body: { channel: "stable" } },
  { method: "setDeploymentPin", suffix: "pin", body: { releaseId: "release_example" } },
]) {
  test(`manager ${scenario.method} accepts an ID and body without changing the HTTP request`, async () => {
    let requests = 0
    const response = { deploymentId: "deployment/example", channel: "stable", releaseId: "release_example" }
    const sdk = new AlienManager({
      bearer: "test-token",
      serverURL: "https://sdk-test.invalid",
      httpClient: new HTTPClient({ fetcher: async request => {
        requests++
        assert.equal(request.method, "PUT")
        assert.equal(new URL(request.url).pathname, `/v1/deployments/deployment%2Fexample/${scenario.suffix}`)
        assert.equal(request.headers.get("Authorization"), "Bearer test-token")
        assert.deepEqual(await request.json(), scenario.body)
        return Response.json(response)
      } }),
    })

    assert.deepEqual(await sdk.deployments[scenario.method]("deployment/example", scenario.body), response)
    assert.equal(requests, 1)
  })
}

test("manager SDK accepts command responses from managers before created was added", () => {
  const parsed = createCommandResponseFromJSON(
    JSON.stringify({
      commandId: "cmd_123",
      inlineAllowedUpTo: 1024,
      next: "poll",
      state: "PENDING",
    }),
  )

  assert.equal(parsed.ok, true)
  if (parsed.ok) assert.equal(parsed.value.created, undefined)
})

test("manager SDK accepts health responses from managers before capability reporting", () => {
  const parsed = healthResponseFromJSON(JSON.stringify({ status: "healthy" }))

  assert.equal(parsed.ok, true)
  if (parsed.ok) assert.equal(parsed.value.operationResultContract, undefined)
})

test("manager SDK sends the observed application with a sync request", () => {
  const body = JSON.parse(
    agentSyncRequestToJSON({
      deploymentId: "dep_123",
      application: {
        source: "kubernetes",
        chartName: "shop",
        chartVersion: "1.4.0",
        images: [
          {
            workload: "apps/v1:Deployment:shop:api",
            container: "api",
            image: "registry.example.com/shop/api:1.4.0",
          },
        ],
        complete: true,
        observedAt: new Date("2026-09-24T10:00:00Z"),
      },
    }),
  )

  assert.deepEqual(body.application, {
    source: "kubernetes",
    chartName: "shop",
    chartVersion: "1.4.0",
    images: [
      {
        workload: "apps/v1:Deployment:shop:api",
        container: "api",
        image: "registry.example.com/shop/api:1.4.0",
      },
    ],
    complete: true,
    observedAt: "2026-09-24T10:00:00.000Z",
  })
})


test("manager compute settings preserve selected per-replica resources on the wire", () => {
  const settings = { containers: { api: { cpu: 1, memory: "1Gi" } } }
  const encoded = computeSettingsToJSON(settings)
  assert.deepEqual(JSON.parse(encoded), settings)
  assert.deepEqual(computeSettingsFromJSON(encoded), { ok: true, value: settings })
})

for (const quotaWarningOverrides of [["L-1216C47A"], []]) {
  test(`manager SDK saves ${quotaWarningOverrides.length ? "overrides" : "an explicit clear"} without retrying`, async () => {
    let requests = 0
    const sdk = new AlienManager({
      bearer: "test-token",
      serverURL: "https://sdk-test.invalid",
      httpClient: new HTTPClient({ fetcher: async request => {
        requests++
        assert.equal(request.method, "PATCH")
        assert.equal(new URL(request.url).pathname, "/v1/deployments/deployment%2Fexample/quota-warning-overrides")
        assert.equal(request.headers.get("Authorization"), "Bearer test-token")
        assert.deepEqual(await request.json(), { quotaWarningOverrides })
        return new Response(null, { status: 204 })
      } }),
    })
    assert.equal(await sdk.deployments.setQuotaWarningOverrides("deployment/example", { quotaWarningOverrides }), undefined)
    assert.equal(requests, 1)
  })
}

test("stack import responses retain resource metadata and accept older managers", async () => {
  const { stackImportResponseFromJSON } = await import("../typescript/esm/models/stackimportresponse.js")
  for (const metadata of [
    {},
    { displayName: "Document storage", description: "Stores uploaded documents." },
    { externalBinding: true },
  ]) {
    const parsed = stackImportResponseFromJSON(JSON.stringify({
      deploymentId: "deployment-example",
      stackSettings: {},
      stackState: {
        platform: "aws",
        resourcePrefix: "example",
        resources: {
          documents: {
            config: { type: "storage", id: "documents" },
            type: "storage",
            status: "pending",
            ...metadata,
          },
        },
      },
    }))
    assert.equal(parsed.ok, true)
    if (parsed.ok) {
      const resource = parsed.value.stackState.resources.documents
      assert.equal(resource.displayName, metadata.displayName)
      assert.equal(resource.description, metadata.description)
      assert.equal(resource.externalBinding, metadata.externalBinding)
      assert.equal(resource.config.id, "documents")
    }
  }
})
