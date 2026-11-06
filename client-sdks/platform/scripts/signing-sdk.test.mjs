import assert from "node:assert/strict";
import test from "node:test";
import { Alien } from "../typescript/esm/sdk/sdk.js";
import { HTTPClient } from "../typescript/esm/lib/http.js";
import { APIError } from "../typescript/esm/models/errors/apierror.js";

function client(fetcher) {
  return new Alien({
    serverURL: "https://sdk-test.invalid",
    apiKey: "ax_ws_test",
    httpClient: new HTTPClient({ fetcher }),
  });
}

const canonicalBytes = Buffer.from('{\n  "message": "café 🛸", "sequence": 7,\n  "whitespace": "  preserved\\n"\n}\n');
const statement = {
  payload: canonicalBytes.toString("base64"),
  signatures: [{ signer: "ci-primary", sig: "MEUCIQAB+/==" }],
};

test("signed statements keep the exact signed bytes through HTTP writes and list replies", async () => {
  const observed = [];
  const sdk = client(async request => {
    const url = new URL(request.url);
    observed.push(`${request.method} ${url.pathname}`);
    if (request.method === "POST") {
      assert.deepEqual(await request.json(), { statement });
      if (url.pathname.endsWith("/signer-lists")) return Response.json({ version: 7 }, { status: 201 });
      return new Response(null, { status: 201 });
    }
    return Response.json({
      items: [statement],
      versions: [{ version: 7, createdBy: "test-ci", createdAt: "2026-11-06T00:00:00Z" }],
    });
  });
  assert.deepEqual(await sdk.signing.addSignerList("example", { statement }), { version: 7 });
  await sdk.signing.addDeploymentConfigStatement("dep_test", { statement });
  const reply = await sdk.signing.listSignerLists({ idOrName: "example" });
  assert.deepEqual(reply.items, [statement]);
  assert.deepEqual(Buffer.from(reply.items[0].payload, "base64"), canonicalBytes);
  assert.equal(reply.versions[0].createdAt.toISOString(), "2026-11-06T00:00:00.000Z");
  assert.deepEqual(observed, [
    "POST /v1/projects/example/signer-lists",
    "POST /v1/deployments/dep_test/config-statements",
    "GET /v1/projects/example/signer-lists",
  ]);
});

test("project signing replies preserve registered device kinds and pending status", async () => {
  const body = {
    fingerprint: `SHA256:${"a".repeat(64)}`,
    listVersion: 7,
    expiresAt: "2027-01-01T00:00:00Z",
    recovered: false,
    devices: ["device", "ci", "recovery"].map(kind => ({ id: kind, label: kind, kind, roles: ["release"] })),
    hasRecovery: true,
    adminCount: 1,
    pendingChanges: [{
      id: "pending-1", projectId: "project-1", status: "pending", summary: "Add device",
      basedOnVersion: 7, body: { previous: "opaque\nbytes", deny: [], recovered: false },
      requestedBy: "test-ci", createdAt: "2026-11-06T00:00:00Z", resolvedAt: null, signerListVersion: null,
    }],
    setupHosting: null,
  };
  const sdk = client(async () => Response.json(body));
  const reply = await sdk.signing.getProjectSigning({ idOrName: "example" });
  assert.deepEqual(reply.devices, body.devices);
  assert.equal(reply.pendingChanges[0].status, "pending");
  assert.deepEqual(reply.pendingChanges[0].body, body.pendingChanges[0].body);
  assert.equal(reply.expiresAt.toISOString(), "2027-01-01T00:00:00.000Z");
  for (const invalid of [
    { ...body, devices: [{ ...body.devices[0], kind: "unknown" }] },
    { ...body, pendingChanges: [{ ...body.pendingChanges[0], status: "unknown" }] },
  ]) {
    await assert.rejects(client(async () => Response.json(invalid)).signing.getProjectSigning({ idOrName: "example" }), { name: "ResponseValidationError" });
  }
});

for (const [name, status, invoke] of [
  ["private instance heartbeat", 403, sdk => sdk.managers.reportHeartbeat("mgr_test", { status: "healthy", url: "https://instance.example", managementConfigs: {} }, { retries: { strategy: "none" } })],
  ["virtual key credential rotation", 500, sdk => sdk.rotateAwsVirtualKeyCredential("key_test", { generation: 2 }, { retries: { strategy: "none" } })],
]) {
  test(`${name} preserves its structured ${status} failure`, async () => {
    const body = { code: "TEST_DENIED", message: "Request refused", retryable: false, internal: false, httpStatusCode: status };
    let count = 0;
    const sdk = client(async () => { count++; return Response.json(body, { status }); });
    await assert.rejects(invoke(sdk), error => {
      assert.ok(error instanceof APIError);
      assert.equal(error.code, body.code);
      assert.equal(error.httpStatusCode, status);
      return true;
    });
    assert.equal(count, 1);
  });
}
