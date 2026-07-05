# Sync

## Overview

Agent sync and state reconciliation

### Available Operations

* [initialize](#initialize) - `POST /v1/initialize` — Inbound: deployment-group bearer (typical),
or workspace bearer for self-hosted operator workflows. New deployments
are created via `DeploymentStore::create_deployment(caller, …)` so
embedders that proxy to an upstream API write the row in the dg's
workspace, not the manager's.
* [agentSync](#agentsync) - `POST /v1/sync` — Inbound: deployment bearer. The agent-driven sync
path; `caller: &Subject` is threaded into the store so embedders see
the agent's own scope.
* [acquire](#acquire) - `POST /v1/sync/acquire` — Inbound: workspace / dg / deployment bearer.
`caller: &Subject` is threaded into `DeploymentStore::acquire` so
embedders can authorize against the inbound caller's scope.
* [reconcile](#reconcile) - `POST /v1/sync/reconcile` — Inbound: workspace / dg / deployment
bearer. `caller: &Subject` is threaded into `DeploymentStore::reconcile`.
* [release](#release) - `POST /v1/sync/release` — Inbound: workspace / dg / deployment bearer.
`caller: &Subject` is threaded into `DeploymentStore::release`.

## initialize

`POST /v1/initialize` — Inbound: deployment-group bearer (typical),
or workspace bearer for self-hosted operator workflows. New deployments
are created via `DeploymentStore::create_deployment(caller, …)` so
embedders that proxy to an upstream API write the row in the dg's
workspace, not the manager's.

### Example Usage

<!-- UsageSnippet language="typescript" operationID="initialize" method="post" path="/v1/initialize" -->
```typescript
import { AlienManager } from "@alienplatform/manager-api";

const alienManager = new AlienManager({
  serverURL: "https://api.example.com",
  bearer: process.env["ALIEN_MANAGER_BEARER"] ?? "",
});

async function run() {
  const result = await alienManager.sync.initialize({});

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienManagerCore } from "@alienplatform/manager-api/core.js";
import { syncInitialize } from "@alienplatform/manager-api/funcs/syncInitialize.js";

// Use `AlienManagerCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alienManager = new AlienManagerCore({
  serverURL: "https://api.example.com",
  bearer: process.env["ALIEN_MANAGER_BEARER"] ?? "",
});

async function run() {
  const res = await syncInitialize(alienManager, {});
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("syncInitialize failed:", res.error);
  }
}

run();
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [models.InitializeRequest](../../models/initializerequest.md)                                                                                                                  | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.InitializeResponse](../../models/initializeresponse.md)\>**

### Errors

| Error Type                      | Status Code                     | Content Type                    |
| ------------------------------- | ------------------------------- | ------------------------------- |
| errors.AlienManagerDefaultError | 4XX, 5XX                        | \*/\*                           |

## agentSync

`POST /v1/sync` — Inbound: deployment bearer. The agent-driven sync
path; `caller: &Subject` is threaded into the store so embedders see
the agent's own scope.

### Example Usage

<!-- UsageSnippet language="typescript" operationID="agent_sync" method="post" path="/v1/sync" -->
```typescript
import { AlienManager } from "@alienplatform/manager-api";

const alienManager = new AlienManager({
  serverURL: "https://api.example.com",
  bearer: process.env["ALIEN_MANAGER_BEARER"] ?? "",
});

async function run() {
  const result = await alienManager.sync.agentSync({
    deploymentId: "<id>",
    observedInventoryBatches: [
      {
        backend: "local",
        complete: false,
        controllerPlatform: "gcp",
        inventoryScope: "<value>",
        observedAt: new Date("2024-02-21T16:58:35.335Z"),
        resources: [
          {
            displayName: "Olin.Feest",
            health: "unhealthy",
            lifecycle: "updating",
            partial: true,
            providerKind: "<value>",
            providerStale: true,
            rawIdentity: "<value>",
            resourceTypeHint: "worker",
          },
        ],
        sourceKind: "<value>",
      },
    ],
    resourceHeartbeats: [
      {
        backend: "kubernetes",
        controllerPlatform: "gcp",
        data: {
          data: {
            path: "/etc/mail",
            pathExists: true,
            secretMetadataListed: false,
            status: {
              collectionIssues: [],
              health: "unhealthy",
              lifecycle: "failed",
              partial: true,
              stale: false,
            },
            backend: "local",
          },
          resourceType: "vault",
        },
        observedAt: new Date("2026-05-14T05:33:22.635Z"),
        raw: [],
        resourceId: "<id>",
        resourceType: "worker",
      },
    ],
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienManagerCore } from "@alienplatform/manager-api/core.js";
import { syncAgentSync } from "@alienplatform/manager-api/funcs/syncAgentSync.js";

// Use `AlienManagerCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alienManager = new AlienManagerCore({
  serverURL: "https://api.example.com",
  bearer: process.env["ALIEN_MANAGER_BEARER"] ?? "",
});

async function run() {
  const res = await syncAgentSync(alienManager, {
    deploymentId: "<id>",
    observedInventoryBatches: [
      {
        backend: "local",
        complete: false,
        controllerPlatform: "gcp",
        inventoryScope: "<value>",
        observedAt: new Date("2024-02-21T16:58:35.335Z"),
        resources: [
          {
            displayName: "Olin.Feest",
            health: "unhealthy",
            lifecycle: "updating",
            partial: true,
            providerKind: "<value>",
            providerStale: true,
            rawIdentity: "<value>",
            resourceTypeHint: "worker",
          },
        ],
        sourceKind: "<value>",
      },
    ],
    resourceHeartbeats: [
      {
        backend: "kubernetes",
        controllerPlatform: "gcp",
        data: {
          data: {
            path: "/etc/mail",
            pathExists: true,
            secretMetadataListed: false,
            status: {
              collectionIssues: [],
              health: "unhealthy",
              lifecycle: "failed",
              partial: true,
              stale: false,
            },
            backend: "local",
          },
          resourceType: "vault",
        },
        observedAt: new Date("2026-05-14T05:33:22.635Z"),
        raw: [],
        resourceId: "<id>",
        resourceType: "worker",
      },
    ],
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("syncAgentSync failed:", res.error);
  }
}

run();
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [models.AgentSyncRequest](../../models/agentsyncrequest.md)                                                                                                                    | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.AgentSyncResponse](../../models/agentsyncresponse.md)\>**

### Errors

| Error Type                      | Status Code                     | Content Type                    |
| ------------------------------- | ------------------------------- | ------------------------------- |
| errors.AlienManagerDefaultError | 4XX, 5XX                        | \*/\*                           |

## acquire

`POST /v1/sync/acquire` — Inbound: workspace / dg / deployment bearer.
`caller: &Subject` is threaded into `DeploymentStore::acquire` so
embedders can authorize against the inbound caller's scope.

### Example Usage

<!-- UsageSnippet language="typescript" operationID="acquire" method="post" path="/v1/sync/acquire" -->
```typescript
import { AlienManager } from "@alienplatform/manager-api";

const alienManager = new AlienManager({
  serverURL: "https://api.example.com",
  bearer: process.env["ALIEN_MANAGER_BEARER"] ?? "",
});

async function run() {
  const result = await alienManager.sync.acquire({
    deploymentModel: "pull",
    session: "<value>",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienManagerCore } from "@alienplatform/manager-api/core.js";
import { syncAcquire } from "@alienplatform/manager-api/funcs/syncAcquire.js";

// Use `AlienManagerCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alienManager = new AlienManagerCore({
  serverURL: "https://api.example.com",
  bearer: process.env["ALIEN_MANAGER_BEARER"] ?? "",
});

async function run() {
  const res = await syncAcquire(alienManager, {
    deploymentModel: "pull",
    session: "<value>",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("syncAcquire failed:", res.error);
  }
}

run();
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [models.AcquireRequest](../../models/acquirerequest.md)                                                                                                                        | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.AcquireResponse](../../models/acquireresponse.md)\>**

### Errors

| Error Type                      | Status Code                     | Content Type                    |
| ------------------------------- | ------------------------------- | ------------------------------- |
| errors.AlienManagerDefaultError | 4XX, 5XX                        | \*/\*                           |

## reconcile

`POST /v1/sync/reconcile` — Inbound: workspace / dg / deployment
bearer. `caller: &Subject` is threaded into `DeploymentStore::reconcile`.

### Example Usage

<!-- UsageSnippet language="typescript" operationID="reconcile" method="post" path="/v1/sync/reconcile" -->
```typescript
import { AlienManager } from "@alienplatform/manager-api";

const alienManager = new AlienManager({
  serverURL: "https://api.example.com",
  bearer: process.env["ALIEN_MANAGER_BEARER"] ?? "",
});

async function run() {
  const result = await alienManager.sync.reconcile({
    deploymentId: "<id>",
    observedInventoryBatches: [
      {
        backend: "external",
        complete: true,
        controllerPlatform: "local",
        inventoryScope: "<value>",
        observedAt: new Date("2024-07-25T23:31:08.887Z"),
        resources: [],
        sourceKind: "<value>",
      },
    ],
    resourceHeartbeats: [
      {
        backend: "external",
        controllerPlatform: "test",
        data: {
          data: {
            namespace: "<value>",
            prefix: "<value>",
            secretMetadataListed: true,
            status: {
              collectionIssues: [
                {
                  message: "<value>",
                  reason: "collection-failed",
                  severity: "warning",
                  source: "<value>",
                },
              ],
              health: "healthy",
              lifecycle: "updating",
              partial: false,
              stale: false,
            },
            backend: "kubernetesSecret",
          },
          resourceType: "vault",
        },
        observedAt: new Date("2025-11-14T09:40:20.690Z"),
        raw: [
          {
            body: "<value>",
            collectedAt: new Date("2024-09-20T22:56:29.622Z"),
            format: "json",
            source: "<value>",
            truncated: false,
          },
        ],
        resourceId: "<id>",
        resourceType: "worker",
      },
    ],
    session: "<value>",
    state: {
      "currentRelease": {
        "releaseId": "<id>",
        "stack": {
          "id": "<id>",
          "resources": {

          },
        },
      },
      "platform": "local",
      "runtimeMetadata": {
        "preparedStack": {
          "id": "<id>",
          "resources": {
            "key": {
              "config": {
                "id": "<id>",
                "type": "function",
              },
              "dependencies": [
                {
                  "id": "<id>",
                  "type": "function",
                },
              ],
              "lifecycle": "live",
            },
          },
        },
      },
      "stackState": {
        "platform": "gcp",
        "resourcePrefix": "<value>",
        "resources": {
          "key": {
            "config": {
              "id": "<id>",
              "type": "function",
            },
            "dependencies": [
              {
                "id": "<id>",
                "type": "function",
              },
            ],
            "error": {
              "code": "NOT_FOUND",
              "internal": false,
              "message": "Item not found.",
              "retryable": false,
            },
            "outputs": {
              "type": "function",
            },
            "previousConfig": {
              "id": "<id>",
              "type": "function",
            },
            "status": "deleted",
            "type": "<value>",
          },
        },
      },
      "status": "deleted",
      "targetRelease": {
        "releaseId": "<id>",
        "stack": {
          "id": "<id>",
          "resources": {

          },
        },
      },
    },
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienManagerCore } from "@alienplatform/manager-api/core.js";
import { syncReconcile } from "@alienplatform/manager-api/funcs/syncReconcile.js";

// Use `AlienManagerCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alienManager = new AlienManagerCore({
  serverURL: "https://api.example.com",
  bearer: process.env["ALIEN_MANAGER_BEARER"] ?? "",
});

async function run() {
  const res = await syncReconcile(alienManager, {
    deploymentId: "<id>",
    observedInventoryBatches: [
      {
        backend: "external",
        complete: true,
        controllerPlatform: "local",
        inventoryScope: "<value>",
        observedAt: new Date("2024-07-25T23:31:08.887Z"),
        resources: [],
        sourceKind: "<value>",
      },
    ],
    resourceHeartbeats: [
      {
        backend: "external",
        controllerPlatform: "test",
        data: {
          data: {
            namespace: "<value>",
            prefix: "<value>",
            secretMetadataListed: true,
            status: {
              collectionIssues: [
                {
                  message: "<value>",
                  reason: "collection-failed",
                  severity: "warning",
                  source: "<value>",
                },
              ],
              health: "healthy",
              lifecycle: "updating",
              partial: false,
              stale: false,
            },
            backend: "kubernetesSecret",
          },
          resourceType: "vault",
        },
        observedAt: new Date("2025-11-14T09:40:20.690Z"),
        raw: [
          {
            body: "<value>",
            collectedAt: new Date("2024-09-20T22:56:29.622Z"),
            format: "json",
            source: "<value>",
            truncated: false,
          },
        ],
        resourceId: "<id>",
        resourceType: "worker",
      },
    ],
    session: "<value>",
    state: {
      "currentRelease": {
        "releaseId": "<id>",
        "stack": {
          "id": "<id>",
          "resources": {
  
          },
        },
      },
      "platform": "local",
      "runtimeMetadata": {
        "preparedStack": {
          "id": "<id>",
          "resources": {
            "key": {
              "config": {
                "id": "<id>",
                "type": "function",
              },
              "dependencies": [
                {
                  "id": "<id>",
                  "type": "function",
                },
              ],
              "lifecycle": "live",
            },
          },
        },
      },
      "stackState": {
        "platform": "gcp",
        "resourcePrefix": "<value>",
        "resources": {
          "key": {
            "config": {
              "id": "<id>",
              "type": "function",
            },
            "dependencies": [
              {
                "id": "<id>",
                "type": "function",
              },
            ],
            "error": {
              "code": "NOT_FOUND",
              "internal": false,
              "message": "Item not found.",
              "retryable": false,
            },
            "outputs": {
              "type": "function",
            },
            "previousConfig": {
              "id": "<id>",
              "type": "function",
            },
            "status": "deleted",
            "type": "<value>",
          },
        },
      },
      "status": "deleted",
      "targetRelease": {
        "releaseId": "<id>",
        "stack": {
          "id": "<id>",
          "resources": {
  
          },
        },
      },
    },
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("syncReconcile failed:", res.error);
  }
}

run();
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [models.ReconcileRequest](../../models/reconcilerequest.md)                                                                                                                    | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.ReconcileResponse](../../models/reconcileresponse.md)\>**

### Errors

| Error Type                      | Status Code                     | Content Type                    |
| ------------------------------- | ------------------------------- | ------------------------------- |
| errors.AlienManagerDefaultError | 4XX, 5XX                        | \*/\*                           |

## release

`POST /v1/sync/release` — Inbound: workspace / dg / deployment bearer.
`caller: &Subject` is threaded into `DeploymentStore::release`.

### Example Usage

<!-- UsageSnippet language="typescript" operationID="release" method="post" path="/v1/sync/release" -->
```typescript
import { AlienManager } from "@alienplatform/manager-api";

const alienManager = new AlienManager({
  serverURL: "https://api.example.com",
  bearer: process.env["ALIEN_MANAGER_BEARER"] ?? "",
});

async function run() {
  await alienManager.sync.release({
    deploymentId: "<id>",
    session: "<value>",
  });


}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienManagerCore } from "@alienplatform/manager-api/core.js";
import { syncRelease } from "@alienplatform/manager-api/funcs/syncRelease.js";

// Use `AlienManagerCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alienManager = new AlienManagerCore({
  serverURL: "https://api.example.com",
  bearer: process.env["ALIEN_MANAGER_BEARER"] ?? "",
});

async function run() {
  const res = await syncRelease(alienManager, {
    deploymentId: "<id>",
    session: "<value>",
  });
  if (res.ok) {
    const { value: result } = res;
    
  } else {
    console.log("syncRelease failed:", res.error);
  }
}

run();
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [models.ReleaseRequest](../../models/releaserequest.md)                                                                                                                        | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<void\>**

### Errors

| Error Type                      | Status Code                     | Content Type                    |
| ------------------------------- | ------------------------------- | ------------------------------- |
| errors.AlienManagerDefaultError | 4XX, 5XX                        | \*/\*                           |