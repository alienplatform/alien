import assert from "node:assert/strict"
import test from "node:test"

import { agentSyncRequestToJSON } from "../typescript/esm/models/agentsyncrequest.js"
import { createCommandResponseFromJSON } from "../typescript/esm/models/createcommandresponse.js"
import { healthResponseFromJSON } from "../typescript/esm/models/healthresponse.js"

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
