import assert from "node:assert/strict"
import test from "node:test"

import { Alien } from "../client-sdks/platform/typescript/esm/index.js"

const managerId = "mgr_enxscjrqiiu2lrc672hwwuc5tv5y"

async function assertTokenCall({ path, requestBody, accessToken, invoke }) {
  const expectedResponse = {
    accessToken,
    expiresIn: 300,
    tokenType: "Bearer",
    managerUrl: "https://manager.example.test",
    databaseId: null,
    controlPlaneUrl: null,
  }
  const requests = []
  const originalFetch = globalThis.fetch

  globalThis.fetch = async (input, init) => {
    const request = input instanceof Request ? input : new Request(input, init)
    requests.push(request.clone())
    return new Response(JSON.stringify(expectedResponse), {
      status: 200,
      headers: { "content-type": "application/json" },
    })
  }

  try {
    const platform = new Alien({
      apiKey: "platform-token",
      serverURL: "https://api.example.test",
    })
    const response = await invoke(platform)

    assert.equal(requests.length, 1)
    const [request] = requests
    assert.ok(request instanceof Request)
    assert.equal(request.method, "POST")
    assert.equal(request.url, `https://api.example.test/v1/managers/${managerId}/${path}`)
    assert.equal(request.headers.get("authorization"), "Bearer platform-token")
    assert.equal(request.headers.get("content-type"), "application/json")
    assert.deepStrictEqual(await request.json(), requestBody)
    assert.deepStrictEqual(response, expectedResponse)
  } finally {
    globalThis.fetch = originalFetch
  }
}

test("generateManagerCommandToken sends an HTTP bearer token and parses the typed response", () => {
  const requestBody = {
    commandId: "cmd_2sxjXxvOYct7IohT3ukliAzf7Nzb",
  }
  return assertTokenCall({
    path: "command-token",
    requestBody,
    accessToken: "manager-command-token",
    invoke: platform =>
      platform.managers.generateManagerCommandToken({
        id: managerId,
        generateManagerCommandTokenRequest: requestBody,
      }),
  })
})

test("generateManagerBindingToken sends only the deployment scope and parses the typed response", () => {
  const requestBody = {
    deploymentId: "dep_2sxjXxvOYct7IohT3ukliAzf7Nzb",
  }
  return assertTokenCall({
    path: "binding-token",
    requestBody,
    accessToken: "manager-binding-token",
    invoke: platform =>
      platform.managers.generateManagerBindingToken({
        id: managerId,
        generateManagerBindingTokenRequest: requestBody,
      }),
  })
})
