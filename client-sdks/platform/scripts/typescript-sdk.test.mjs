import assert from "node:assert/strict";
import test from "node:test";
import { HTTPClient } from "../typescript/esm/lib/http.js";
import { Alien } from "../typescript/esm/sdk/sdk.js";

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
    const result = await sdk[scenario.operation]({
      id: deploymentId,
      ...(scenario.body ? { requestBody: scenario.body } : {}),
    });
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
