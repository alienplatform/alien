import assert from "node:assert/strict"
import test from "node:test"
import fs from "node:fs"
import path from "node:path"
import { fileURLToPath } from "node:url"
import { spawnSync } from "node:child_process"

import { agentSyncRequestToJSON } from "../typescript/esm/models/agentsyncrequest.js"
import { createCommandResponseFromJSON } from "../typescript/esm/models/createcommandresponse.js"
import { healthResponseFromJSON } from "../typescript/esm/models/healthresponse.js"
import { managerInfoResponseFromJSON } from "../typescript/esm/models/managerinforesponse.js"
import { stackSettingsFromJSON, stackSettingsToJSON } from "../typescript/esm/models/stacksettings.js"

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


test("manager SDK preserves optional setup support and its conservative fallback", () => {
  for (const support of [undefined, false, true]) {
    const capabilities = { tunnels: true, charts: false }
    if (support !== undefined) capabilities.awsSetupNodeIdentity = support
    const parsed = managerInfoResponseFromJSON(JSON.stringify({
      url: "https://manager.example.com",
      registryHost: "manager.example.com",
      version: "0.1.0",
      capabilities,
    }))
    assert.equal(parsed.ok, true)
    if (parsed.ok) {
      assert.equal(parsed.value.capabilities.awsSetupNodeIdentity, support)
      assert.equal(parsed.value.capabilities.awsSetupNodeIdentity ?? false, support ?? false)
      assert.equal(parsed.value.capabilities.tunnels, true)
      assert.equal(parsed.value.capabilities.charts, false)
    }
  }
})

test("manager SDK round-trips typed storage binding settings without dropping connection fields", () => {
  const settings = {
    externalBindings: {
      archive: {
        type: "storage",
        service: "s3",
        bucketName: "archive-bucket",
        endpoint: "https://storage.example.com",
        region: "us-east-1",
        forcePathStyle: true,
        accessKeyId: "example-access-key",
        secretAccessKey: { secretRef: { name: "storage-auth", key: "secret" } },
      },
    },
  }
  const parsed = stackSettingsFromJSON(JSON.stringify(settings))
  assert.equal(parsed.ok, true)
  if (parsed.ok) assert.deepEqual(JSON.parse(stackSettingsToJSON(parsed.value)), settings)
})


test("existing manager model imports and capability literals still typecheck", () => {
  const sdkDirectory = fileURLToPath(new URL("../typescript/", import.meta.url))
  const directory = fs.mkdtempSync(path.join(sdkDirectory, ".sdk-consumer-"))
  try {
    const consumer = path.join(directory, "consumer.ts")
    fs.writeFileSync(consumer, `
import {
  type ManagerCapabilities,
  type ExternalBindings,
  externalBindingsFromJSON,
  externalBindingsToJSON,
} from "@alienplatform/manager-api/models";

const capabilities: ManagerCapabilities = { charts: false, tunnels: true };
const bindings: ExternalBindings = {
  archive: { type: "storage", service: "s3", bucketName: "archive-bucket" },
};
const parsed = externalBindingsFromJSON(externalBindingsToJSON(bindings));
if (parsed.ok) externalBindingsToJSON(parsed.value);
void capabilities;
`)
    const result = spawnSync(process.execPath, [
      path.join(sdkDirectory, "node_modules/typescript/bin/tsc"),
      "--strict", "--skipLibCheck", "--noEmit",
      "--module", "NodeNext", "--moduleResolution", "NodeNext",
      "--target", "ES2022", consumer,
    ], { encoding: "utf8" })
    assert.equal(result.status, 0, result.stdout + result.stderr)
  } finally {
    fs.rmSync(directory, { recursive: true, force: true })
  }
})
