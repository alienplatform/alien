import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";
import test from "node:test";
import { APIError } from "../typescript/esm/models/errors/apierror.js";
import { HTTPClient } from "../typescript/esm/lib/http.js";
import { Alien } from "../typescript/esm/sdk/sdk.js";
import { deploymentGroupsCreateToken } from "../typescript/esm/funcs/deploymentGroupsCreateToken.js";
import { CreateAccessRequestMaxRisk } from "../typescript/esm/models/index.js";
import {
  KubernetesPermissions$outboundSchema,
  Rule$outboundSchema,
  Verb,
  kubernetesPermissionsToJSON,
  ruleToJSON,
} from "../typescript/esm/models/publishoperationspluginrequest.js";
import { PublishOperationsPluginResponse$inboundSchema } from "../typescript/esm/models/publishoperationspluginresponse.js";
import { DeploymentInfoHelm$inboundSchema } from "../typescript/esm/models/deploymentinfo.js";
import { PackageRule$inboundSchema, packageRuleFromJSON } from "../typescript/esm/models/package.js";
import {
  UpdateProjectBinaryTarget,
  UpdateProjectHelm$outboundSchema,
  updateProjectHelmToJSON,
} from "../typescript/esm/models/updateproject.js";

test("published package model imports still validate and serialize", () => {
  const rule = { apiGroup: "apps", resource: "deployments", verbs: ["get"], reason: "Read workload state" };
  assert.deepEqual(PackageRule$inboundSchema.parse(rule), rule);
  assert.deepEqual(packageRuleFromJSON(JSON.stringify(rule)), { ok: true, value: rule });
  const helm = { enabled: true, chartName: "application", description: "Application deployment" };
  assert.deepEqual(UpdateProjectHelm$outboundSchema.parse(helm), helm);
  assert.deepEqual(JSON.parse(updateProjectHelmToJSON(helm)), helm);
  assert.equal(UpdateProjectBinaryTarget.LinuxArm64, "linux-arm64");
});

test("Helm installation routing survives SDK response validation", () => {
  const helm = {
    status: "ready",
    chartRef: "oci://registry.example/charts/application",
    outputs: {
      chart: "oci://registry.example/charts/application",
      version: "1.2.3",
      managerUrl: "https://manager.example",
    },
    managerUrlOverride: "https://custom-manager.example",
  };
  assert.deepEqual(DeploymentInfoHelm$inboundSchema.parse(helm), helm);
  const previous = {
    ...helm,
    outputs: { chart: helm.outputs.chart, version: helm.outputs.version },
  };
  delete previous.managerUrlOverride;
  assert.deepEqual(DeploymentInfoHelm$inboundSchema.parse(previous), previous);
});

// Run after pnpm -C client-sdks/platform/typescript build. Exercise the shipped
// JavaScript, including request serialization and response validation.
const deploymentId = `dep_${"a".repeat(28)}`;
const rotation = {
  id: "rotation_test",
  revision: 1,
  status: "prepared",
  expiresAt: "2026-09-09T12:00:00.000Z",
};
const event = {
  id: `event_${"a".repeat(28)}`,
  deploymentId,
  projectId: `prj_${"a".repeat(28)}`,
  workspaceId: `ws_${"a".repeat(24)}`,
  createdAt: "2026-09-08T12:00:00.000Z",
  state: "success",
};

function client(fetcher) {
  return new Alien({
    serverURL: "https://sdk-test.invalid",
    apiKey: "ax_ws_test",
    workspace: "test-workspace",
    httpClient: new HTTPClient({ fetcher }),
  });
}

test("deployment-group tokens accept an ID and body without changing the HTTP request", async () => {
  const body = {
    description: "Deployment setup",
    expiresAt: new Date("2027-01-01T00:00:00Z"),
    setupItems: [{ item: "deployment", required: true }],
  };
  let requests = 0;
  const sdk = client(async request => {
    requests++;
    const url = new URL(request.url);
    assert.equal(request.method, "POST");
    assert.equal(url.pathname, "/v1/deployment-groups/group%2Fexample/tokens");
    assert.equal(url.searchParams.get("workspace"), "test-workspace");
    assert.equal(request.headers.get("Authorization"), "Bearer ax_ws_test");
    assert.equal(request.headers.get("x-test"), "request-options");
    assert.deepEqual(await request.json(), {
      ...body,
      expiresAt: "2027-01-01T00:00:00.000Z",
    });
    return Response.json({ token: "ax_dg_test", deploymentLink: "https://example.com/setup" });
  });

  const options = {
    fetchOptions: { headers: { "x-test": "request-options" } },
  };
  const result = await sdk.deploymentGroups.createToken("group/example", body, options);
  assert.equal(result.token, "ax_dg_test");
  const standalone = await deploymentGroupsCreateToken(sdk, "group/example", body, options);
  assert.equal(standalone.ok, true);
  assert.deepEqual(standalone.value, result);
  assert.equal(requests, 2);
});

test("command retry keys survive configured transport retries and separate method calls", async () => {
  const request = {
    deploymentId,
    target: "api",
    name: "reindex",
    params: { full: true },
    idempotencyKey: randomUUID(),
    deadline: new Date("2026-10-06T12:00:00.000Z"),
  };
  const sent = [];
  const command = {
    id: `cmd_${"a".repeat(28)}`,
    projectId: `prj_${"a".repeat(28)}`,
    deploymentModel: "pull",
    target: { resourceId: "api", resourceType: "worker" },
    deliveryMode: "pull",
    operationResultContractPersisted: false,
  };
  const sdk = client(async outgoing => {
    assert.equal(outgoing.method, "POST");
    assert.equal(new URL(outgoing.url).pathname, "/v1/commands");
    sent.push(await outgoing.json());
    if (sent.length === 1) {
      return new Response("temporarily unavailable", {
        status: 503,
        headers: { "retry-after-ms": "1" },
      });
    }
    return Response.json(command, { status: 201 });
  });

  assert.deepEqual(await sdk.commands.create(request, {
    retries: {
      strategy: "backoff",
      backoff: {
        initialInterval: 1,
        maxInterval: 5,
        exponent: 1,
        maxElapsedTime: 1_000,
      },
    },
  }), command);
  const serialized = JSON.parse(JSON.stringify(request));
  assert.deepEqual(sent, [serialized, serialized]);

  await sdk.commands.create(request);
  const saved = JSON.parse(JSON.stringify(request));
  const restored = {
    ...saved,
    deadline: saved.deadline == null ? saved.deadline : new Date(saved.deadline),
  };
  await sdk.commands.create(restored);
  const next = { ...request, idempotencyKey: randomUUID() };
  await sdk.commands.create(next);
  assert.deepEqual(sent, [serialized, serialized, serialized, serialized, {
    ...serialized, idempotencyKey: next.idempotencyKey,
  }]);
});

test("legacy publish-plugin deep imports preserve Kubernetes permission exports", () => {
  const rule = {
    apiGroup: "apps",
    resource: "deployments",
    verbs: [Verb.Get],
    reason: "Read deployment state",
  };
  const permissions = { schemaVersion: 1, rules: [rule] };

  assert.deepEqual(Rule$outboundSchema.parse(rule), rule);
  assert.equal(ruleToJSON(rule), JSON.stringify(rule));
  assert.deepEqual(KubernetesPermissions$outboundSchema.parse(permissions), permissions);
  assert.equal(kubernetesPermissionsToJSON(permissions), JSON.stringify(permissions));
});

test("operations plugin responses retain the published identity", () => {
  const published = { name: "registry", version: "1", tier: "mutating" };
  assert.deepEqual(PublishOperationsPluginResponse$inboundSchema.parse(published), published);
});

test("existing access request risk enum imports remain compatible", () => {
  assert.equal(CreateAccessRequestMaxRisk.ReadOnly, "read-only");
});

test("configured server query parameters survive operation globals", async () => {
  const sdk = new Alien({
    serverURL: "https://sdk-test.invalid/proxy?token=preserved&workspace=stale",
    apiKey: "ax_ws_test",
    workspace: "test-workspace",
    httpClient: new HTTPClient({
      fetcher: async request => {
        const url = new URL(request.url);
        assert.equal(url.pathname, "/proxy/v1/events");
        assert.equal(url.searchParams.get("token"), "preserved");
        assert.equal(url.searchParams.get("workspace"), "test-workspace");
        return Response.json({ items: [], nextCursor: null });
      },
    }),
  });

  await sdk.events.list();
});

test("retry-after-ms and timeoutMs apply independently to each attempt", async () => {
  const delays = [];
  const signals = [];
  const originalSetTimeout = globalThis.setTimeout;
  globalThis.setTimeout = (callback, delay, ...args) => {
    delays.push(delay);
    queueMicrotask(() => callback(...args));
    return 0;
  };
  try {
    let requests = 0;
    const sdk = client(async request => {
      requests++;
      signals.push(request.signal);
      if (requests === 1) {
        return new Response("retry", {
          status: 503,
          headers: { "retry-after-ms": "17" },
        });
      }
      return Response.json({ ...event, data: { type: "Finished" } });
    });

    await sdk.events.get(
      { id: event.id },
      {
        timeoutMs: 1_000,
        retries: {
          strategy: "backoff",
          backoff: {
            initialInterval: 10_000,
            maxInterval: 10_000,
            exponent: 1,
            maxElapsedTime: 30_000,
          },
        },
      },
    );

    assert.equal(requests, 2);
    assert.deepEqual(delays, [17]);
    assert.notEqual(signals[0], signals[1]);
    assert.equal(signals.every(signal => !signal.aborted), true);
  } finally {
    globalThis.setTimeout = originalSetTimeout;
  }
});

for (const data of [
  { type: "Finished" },
  ...["prepared", "completed", "cancelled"].map(status => ({
    type: "DeploymentCredentialRotation",
    deploymentId,
    rotationId: rotation.id,
    revision: rotation.revision,
    status,
    previousKeyId: "key_previous",
    candidateKeyId: "key_candidate",
    actor: { kind: "user", id: "user_test" },
  })),
]) {
  for (const operation of ["get", "list"]) {
    test(`Events.${operation} reads ${data.status ?? data.type}`, async () => {
      let requests = 0;
      const sdk = client(async request => {
        requests++;
        assert.equal(request.method, "GET");
        assert.equal(
          new URL(request.url).pathname,
          operation === "get" ? `/v1/events/${event.id}` : "/v1/events",
        );
        return Response.json(operation === "get"
          ? { ...event, data }
          : { items: [{ ...event, data }], nextCursor: null });
      });
      const result = operation === "get"
        ? await sdk.events.get({ id: event.id })
        : (await sdk.events.list()).items[0];
      assert.deepEqual(result.data, data);
      assert.equal(requests, 1);
    });
  }
}

for (const scenario of [
  {
    operation: "getDeploymentCredentialRotation",
    method: "GET",
    suffix: "",
    response: { deploymentId, revision: 1, rotation },
  },
  {
    operation: "prepareDeploymentCredentialRotation",
    method: "POST",
    suffix: "",
    body: { expectedRevision: 0 },
    response: rotation,
  },
  {
    operation: "cancelDeploymentCredentialRotation",
    method: "POST",
    suffix: "/cancel",
    body: { rotationId: rotation.id },
    response: { ...rotation, status: "cancelled", revision: 2 },
  },
  {
    operation: "getDeploymentCredentialRotationValues",
    method: "POST",
    suffix: "/values",
    body: { rotationId: rotation.id },
    response: { deploymentId, revision: 1, token: "ax_dep_test" },
  },
]) {
  test(`${scenario.operation} preserves its wire contract`, async () => {
    let requests = 0;
    const sdk = client(async request => {
      requests++;
      const url = new URL(request.url);
      assert.equal(request.method, scenario.method);
      assert.equal(url.pathname, `/v1/deployments/${deploymentId}/credential-rotation${scenario.suffix}`);
      assert.equal(url.searchParams.get("workspace"), "test-workspace");
      assert.equal(request.headers.get("Authorization"), "Bearer ax_ws_test");
      if (scenario.body) assert.deepEqual(await request.json(), scenario.body);
      else assert.equal(await request.text(), "");
      return Response.json(scenario.response);
    });
    const result = scenario.body
      ? await sdk[scenario.operation](deploymentId, scenario.body)
      : await sdk[scenario.operation]({ id: deploymentId });
    assert.deepEqual(result, scenario.response);
    assert.equal(requests, 1);
  });
}

test("Events.get still rejects malformed rotation events", async () => {
  const sdk = client(async () => Response.json({
    ...event,
    data: { type: "DeploymentCredentialRotation", status: "prepared" },
  }));
  await assert.rejects(sdk.events.get({ id: event.id }), {
    name: "ResponseValidationError",
  });
});

const commandId = "cmd_2sxjXxvOYct7IohT3ukliAzfmpqr";
for (const [operation, args] of [
  ["resolveTarget", [{ deploymentId, command: "reindex" }]],
  ["get", [{ id: commandId }]],
  ["update", [commandId, { state: "DISPATCHED" }]],
  ["dispatch", [commandId, { dispatchedAt: new Date() }]],
  ["complete", [commandId, { state: "SUCCEEDED", completedAt: new Date() }]],
  ["incrementAttempt", [{ id: commandId }]],
]) {
  test(`Commands.${operation} preserves a structured 503 API error`, async () => {
    let requests = 0;
    const body = {
      code: "SERVICE_UNAVAILABLE",
      message: "The API cannot serve the request right now.",
      retryable: true,
      internal: false,
      httpStatusCode: 503,
      requestId: "00000000-0000-4000-8000-000000000000",
    };
    const sdk = client(async () => {
      requests++;
      return Response.json(body, { status: 503 });
    });
    await assert.rejects(
      sdk.commands[operation](...args, { retries: { strategy: "none" } }),
      error => {
        assert.ok(error instanceof APIError);
        assert.equal(error.code, body.code);
        assert.equal(error.message, body.message);
        assert.equal(error.retryable, body.retryable);
        assert.equal(error.httpStatusCode, body.httpStatusCode);
        assert.equal(error.requestId, body.requestId);
        return true;
      },
    );
    assert.equal(requests, 1);
  });
}


test("existing access request risk enum deep imports remain compatible", async () => {
  const { CreateAccessRequestMaxRisk } = await import("../typescript/esm/models/createaccessrequest.js");
  assert.equal(CreateAccessRequestMaxRisk.ReadOnly, "read-only");
});
