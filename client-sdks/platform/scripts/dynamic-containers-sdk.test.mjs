import assert from "node:assert/strict"
import test from "node:test"
import { HTTPClient } from "../typescript/esm/lib/http.js"
import { Alien } from "../typescript/esm/sdk/sdk.js"

test("generated dynamic container client sends a scoped write and parses status", async () => {
  const requests = []
  const sdk = new Alien({
    serverURL: "https://api.example.invalid",
    apiKey: "ax_ws_test",
    workspace: "example",
    httpClient: new HTTPClient({ fetcher: async request => {
      requests.push(request)
      return Response.json({
        name: "agent", generation: 1, appliedGeneration: null,
        status: "pending", statusMessage: null, deleted: false,
        internalServices: [], createdAt: "2026-09-26T00:00:00.000Z",
        updatedAt: "2026-09-26T00:00:00.000Z",
        spec: {
          image: `registry.example.com/agent@sha256:${"a".repeat(64)}`,
          resources: { cpu: "1", memory: "512Mi" },
          replicas: 1, ports: [4111], env: {},
        },
      }, { status: 202 })
    } }),
  })

  const result = await sdk.dynamicContainers.put({
    id: "dep_example", name: "agent",
    putDynamicContainerRequest: {
      spec: {
        image: `registry.example.com/agent@sha256:${"a".repeat(64)}`,
        resources: { cpu: "1", memory: "512Mi" },
        replicas: 1, ports: [4111], env: {},
      },
      secretEnv: { TOKEN: "secret" },
    },
  })
  assert.equal(result.status, "pending")
  assert.equal(requests.length, 1)
  assert.equal(requests[0].method, "PUT")
  const url = new URL(requests[0].url)
  assert.equal(url.pathname, "/v1/deployments/dep_example/dynamic-containers/agent")
  assert.equal(url.searchParams.get("workspace"), "example")
  const body = JSON.parse(await requests[0].text())
  assert.equal(body.secretEnv.TOKEN, "secret")
})
