# Sync

## Overview

### Available Operations

* [list](#list) - List full deployment records for manager operational loops. This endpoint is intentionally separate from the public deployments list, which returns lightweight UI rows.
* [context](#context) - Get computed deployment state and configuration for a manager-side operation without acquiring the deployment reconciliation lock.
* [acquire](#acquire) - Acquire a batch of deployments for processing. Used by Manager to atomically lock deployments matching filters. Each deployment in the batch must be released after processing.
* [reconcile](#reconcile) - Reconcile deployment state. Push model requests that include a session verify lock ownership. Pull model state reports are accepted as authz-gated agent progress even when they carry an agent-sync session. Accepts full DeploymentState after step() execution.
* [release](#release) - Release a deployment lock. Must be called after processing an acquired deployment, even if processing failed. This is critical to avoid deadlocks.

## list

List full deployment records for manager operational loops. This endpoint is intentionally separate from the public deployments list, which returns lightweight UI rows.

### Example Usage

<!-- UsageSnippet language="typescript" operationID="syncList" method="post" path="/v1/sync/list" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.sync.list({
    workspace: "my-workspace",
    syncListRequest: {
      managerId: "mgr_enxscjrqiiu2lrc672hwwuc5",
      deploymentIds: [
        "dep_0c29fq4a2yjb7kx3smwdgxlc",
      ],
      deploymentGroupId: "dg_r27ict8c7vcgsumpj90ackf7b",
    },
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { syncList } from "@alienplatform/platform-api/funcs/syncList.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await syncList(alien, {
    workspace: "my-workspace",
    syncListRequest: {
      managerId: "mgr_enxscjrqiiu2lrc672hwwuc5",
      deploymentIds: [
        "dep_0c29fq4a2yjb7kx3smwdgxlc",
      ],
      deploymentGroupId: "dg_r27ict8c7vcgsumpj90ackf7b",
    },
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("syncList failed:", res.error);
  }
}

run();
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.SyncListRequest](../../models/operations/synclistrequest.md)                                                                                                       | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.SyncListResponse](../../models/synclistresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 400                      | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## context

Get computed deployment state and configuration for a manager-side operation without acquiring the deployment reconciliation lock.

### Example Usage

<!-- UsageSnippet language="typescript" operationID="syncContext" method="post" path="/v1/sync/context" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.sync.context({
    workspace: "my-workspace",
    syncContextRequest: {
      managerId: "mgr_enxscjrqiiu2lrc672hwwuc5",
      deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
    },
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { syncContext } from "@alienplatform/platform-api/funcs/syncContext.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await syncContext(alien, {
    workspace: "my-workspace",
    syncContextRequest: {
      managerId: "mgr_enxscjrqiiu2lrc672hwwuc5",
      deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
    },
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("syncContext failed:", res.error);
  }
}

run();
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.SyncContextRequest](../../models/operations/synccontextrequest.md)                                                                                                 | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.SyncAcquireResponseDeployment](../../models/syncacquireresponsedeployment.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 404                      | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## acquire

Acquire a batch of deployments for processing. Used by Manager to atomically lock deployments matching filters. Each deployment in the batch must be released after processing.

### Example Usage

<!-- UsageSnippet language="typescript" operationID="syncAcquire" method="post" path="/v1/sync/acquire" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.sync.acquire({
    workspace: "my-workspace",
    syncAcquireRequest: {
      managerId: "mgr_enxscjrqiiu2lrc672hwwuc5",
      session: "<value>",
      deploymentIds: [
        "dep_0c29fq4a2yjb7kx3smwdgxlc",
      ],
      deploymentModel: "pull",
    },
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { syncAcquire } from "@alienplatform/platform-api/funcs/syncAcquire.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await syncAcquire(alien, {
    workspace: "my-workspace",
    syncAcquireRequest: {
      managerId: "mgr_enxscjrqiiu2lrc672hwwuc5",
      session: "<value>",
      deploymentIds: [
        "dep_0c29fq4a2yjb7kx3smwdgxlc",
      ],
      deploymentModel: "pull",
    },
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
| `request`                                                                                                                                                                      | [operations.SyncAcquireRequest](../../models/operations/syncacquirerequest.md)                                                                                                 | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.SyncAcquireResponse](../../models/syncacquireresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 400                      | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## reconcile

Reconcile deployment state. Push model requests that include a session verify lock ownership. Pull model state reports are accepted as authz-gated agent progress even when they carry an agent-sync session. Accepts full DeploymentState after step() execution.

### Example Usage

<!-- UsageSnippet language="typescript" operationID="syncReconcile" method="post" path="/v1/sync/reconcile" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.sync.reconcile({
    workspace: "my-workspace",
    syncReconcileRequest: {
      deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
      state: {
        platform: "aws",
        protocolVersion: 149716,
        status: "provisioning-failed",
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
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { syncReconcile } from "@alienplatform/platform-api/funcs/syncReconcile.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await syncReconcile(alien, {
    workspace: "my-workspace",
    syncReconcileRequest: {
      deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
      state: {
        platform: "aws",
        protocolVersion: 149716,
        status: "provisioning-failed",
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
| `request`                                                                                                                                                                      | [operations.SyncReconcileRequest](../../models/operations/syncreconcilerequest.md)                                                                                             | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.SyncReconcileResponse](../../models/syncreconcileresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 404, 409                 | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## release

Release a deployment lock. Must be called after processing an acquired deployment, even if processing failed. This is critical to avoid deadlocks.

### Example Usage

<!-- UsageSnippet language="typescript" operationID="syncRelease" method="post" path="/v1/sync/release" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.sync.release({
    workspace: "my-workspace",
    syncReleaseRequest: {
      deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
      session: "<value>",
    },
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { syncRelease } from "@alienplatform/platform-api/funcs/syncRelease.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await syncRelease(alien, {
    workspace: "my-workspace",
    syncReleaseRequest: {
      deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
      session: "<value>",
    },
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("syncRelease failed:", res.error);
  }
}

run();
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.SyncReleaseRequest](../../models/operations/syncreleaserequest.md)                                                                                                 | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[operations.SyncReleaseResponse](../../models/operations/syncreleaseresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 404                      | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |