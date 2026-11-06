import assert from "node:assert/strict";
import test from "node:test";
import { Alien } from "../typescript/esm/sdk/sdk.js";
import { HTTPClient } from "../typescript/esm/lib/http.js";
import { APIError } from "../typescript/esm/models/errors/apierror.js";
import { generateKeyPairSync, sign, verify } from "node:crypto";

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

test("command result discovery sends confirmation metadata and leaves private retrieval to the caller", async () => {
  const id = `cmd_${"a".repeat(28)}`;
  const connection = {
    managerUrl: "https://instance.example",
    token: "command-scoped-test-token",
    commandId: id,
    operationResultContract: { kind: "json", version: 1 },
  };
  let requests = 0;
  const sdk = client(async request => {
    requests++;
    assert.equal(request.method, "POST");
    assert.equal(request.url, `https://sdk-test.invalid/v1/commands/${id}/connection`);
    assert.deepEqual(await request.json(), { confirmSensitiveOutput: true });
    return Response.json(connection);
  });
  assert.deepEqual(await sdk.commands.resolveResultConnection(id, { confirmSensitiveOutput: true }), connection);
  assert.equal(requests, 1, "discovery must not fetch the private result or follow its endpoint");
});

for (const status of [403, 409, 503]) {
  test(`command result discovery preserves structured ${status} refusal without a fallback request`, async () => {
    const body = { code: "COMMAND_RESULT_UNAVAILABLE", message: "Selected instance cannot serve this result", retryable: status === 503, internal: false, httpStatusCode: status };
    let requests = 0;
    const sdk = client(async () => {
      requests++;
      return Response.json(body, { status });
    });
    await assert.rejects(sdk.commands.resolveResultConnection(`cmd_${"a".repeat(28)}`, { confirmSensitiveOutput: false }, { retries: { strategy: "none" } }), error => {
      assert.ok(error instanceof APIError);
      assert.equal(error.code, body.code);
      assert.equal(error.httpStatusCode, status);
      return true;
    });
    assert.equal(requests, 1);
  });
}

test("signed statement transport preserves a verifiable device signature", async () => {
  const { privateKey, publicKey } = generateKeyPairSync("ec", { namedCurve: "prime256v1" });
  const signature = sign("sha256", canonicalBytes, privateKey);
  const signed = { payload: canonicalBytes.toString("base64"), signatures: [{ signer: "device-primary", sig: signature.toString("base64") }] };
  const sdk = client(async request => {
    const body = await request.json();
    assert.deepEqual(body.statement, signed);
    const transported = Buffer.from(body.statement.payload, "base64");
    assert.deepEqual(transported, canonicalBytes);
    assert.ok(verify("sha256", transported, publicKey, Buffer.from(body.statement.signatures[0].sig, "base64")));
    return Response.json({ version: 7 }, { status: 201 });
  });
  await sdk.signing.addSignerList("example", { statement: signed });
});

for (const retry of [false, true]) {
  test(`private instance ${retry ? "retry" : "creation"} retains nested protected setup settings`, async () => {
    // This is the current public producer's CloudFormation setup response shape.
    const stackSettings = {
      airgapped: true,
      signing: { channel: "production", fingerprint: `SHA256:${"a".repeat(64)}`, privateMachineApiUrl: "https://instance.example/machine" },
      domains: { customDomains: { application: { domain: "app.example", certificate: { managed: {} }, endpoints: { https: "web" } } } },
    };
    const response = {
      managerId: "mgr_test", setupStatus: "pending", setupToken: "test-only-token", setupTokenId: "token_test",
      deploymentLink: "https://setup.example/instance",
      setupConfig: { metadata: { test: true }, policy: { allowedPlatforms: ["aws"], allowedSetupMethods: ["cloudformation"] }, environmentVariables: [] },
      setup: { method: "cloudformation", deploymentPortalUrl: "https://setup.example/instance", launchUrl: "https://console.aws.amazon.com/cloudformation/", templateUrl: "https://packages.example/template.yaml", stackName: "test-instance", region: "us-east-1", stackSettings },
      ...(retry ? { mode: "setup" } : {}),
    };
    const sdk = client(async request => {
      assert.equal(request.method, "POST");
      assert.equal(new URL(request.url).pathname, retry ? "/v1/managers/mgr_test/retry" : "/v1/managers");
      if (!retry) assert.deepEqual(await request.json(), { name: "test-instance", cloud: "aws", region: "us-east-1", setupMethod: "cloudformation" });
      return Response.json(response, { status: retry ? 200 : 201 });
    });
    const reply = retry
      ? await sdk.managers.retry({ id: "mgr_test" })
      : await sdk.managers.create({ name: "test-instance", cloud: "aws", region: "us-east-1", setupMethod: "cloudformation" });
    assert.deepEqual(reply.setup.stackSettings, stackSettings);
    assert.deepEqual(reply.setupConfig, response.setupConfig);
    assert.equal(reply.managerId, response.managerId);
    if (retry) assert.equal(reply.mode, "setup");
  });
}

test("published secret-reference object aliases retain their wire validation", async () => {
  const models = await import("../typescript/esm/models/index.js");
  const deep = await import("../typescript/esm/models/externalbindingecr.js");
  const valid = { secretRef: { key: "credential", name: "application-secrets" } };
  for (const [module, names] of [[models, ["ClusterEndpoint", "PasswordSecretArn", "DataDir1", "ConnectionUrl"]], [deep, ["ClusterEndpoint", "PasswordSecretArn"]]]) {
    for (const name of names) {
      assert.deepEqual(module[`${name}$inboundSchema`].parse(valid), valid);
      assert.deepEqual(module[`${name}$outboundSchema`].parse(valid), valid);
      for (const invalid of [{}, { secretRef: {} }, { secretRef: { key: 7, name: "application-secrets" } }, { secretRef: { key: "credential", name: 7 } }]) {
        assert.equal(module[`${name}$inboundSchema`].safeParse(invalid).success, false);
        assert.equal(module[`${name}$outboundSchema`].safeParse(invalid).success, false);
      }
    }
  }
});

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
