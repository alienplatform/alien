import assert from "node:assert/strict";
import test from "node:test";

import { AlienManager } from "../client-sdks/manager/typescript/esm/index.js";

test("resolveBinding sends an HTTP bearer token and parses the typed response", async () => {
  const expectedRequest = {
    deploymentId: "deployment-123",
    resourceId: "storage-456",
  };
  const expectedResponse = {
    binding: {
      bucketName: "alien-remote-storage",
    },
    clientConfig: {
      accountId: "123456789012",
      credentials: {
        accessKeyId: "ASIAEXAMPLE",
        expiresAt: "2026-07-21T14:30:00Z",
        secretAccessKey: "secret-access-key",
        sessionToken: "session-token",
        type: "sessionCredentials",
      },
      region: "ap-northeast-1",
    },
    expiresAt: "2026-07-21T14:30:00Z",
    service: "s3",
  };
  const requests = [];
  const originalFetch = globalThis.fetch;

  globalThis.fetch = async (input, init) => {
    const request = input instanceof Request ? input : new Request(input, init);
    requests.push(request.clone());
    return new Response(JSON.stringify(expectedResponse), {
      status: 200,
      headers: { "content-type": "application/json" },
    });
  };

  try {
    const manager = new AlienManager({
      bearer: "manager-token",
      serverURL: "https://manager.example.test",
    });

    const response = await manager.bindings.resolveBinding(expectedRequest);

    assert.equal(requests.length, 1);
    const [request] = requests;
    assert.ok(request instanceof Request);
    assert.equal(request.method, "POST");
    assert.equal(request.url, "https://manager.example.test/v1/bindings/resolve");
    assert.equal(request.headers.get("authorization"), "Bearer manager-token");
    assert.equal(request.headers.get("content-type"), "application/json");
    assert.deepStrictEqual(await request.json(), expectedRequest);
    assert.deepStrictEqual(response, expectedResponse);
    assert.equal(response.service, "s3");
    assert.equal(response.clientConfig.credentials.type, "sessionCredentials");
  } finally {
    globalThis.fetch = originalFetch;
  }
});
