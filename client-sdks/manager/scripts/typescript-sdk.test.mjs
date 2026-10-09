import assert from "node:assert/strict"
import test from "node:test"
import { HTTPClient } from "../typescript/esm/lib/http.js"
import { AlienManager } from "../typescript/esm/sdk/sdk.js"

import { agentSyncRequestToJSON } from "../typescript/esm/models/agentsyncrequest.js"
import { createCommandResponseFromJSON } from "../typescript/esm/models/createcommandresponse.js"
import { healthResponseFromJSON } from "../typescript/esm/models/healthresponse.js"

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
