# Deployment

## Overview

### Available Operations

* [getInfo](#getinfo) - Get deployment information for the deployment portal. Accepts both deployment-scoped and deployment-group-scoped API keys. Returns project information, package status/outputs, and either deployment or deployment group details depending on the token type. Poll this endpoint to check if packages are ready.
* [planCompute](#plancompute) - Plan deployment compute for the active release before stack preparation. The response contains recommended machine and scale choices for cloud compute pools.
* [prepareStack](#preparestack) - Prepare the active release stack for a deployment portal setup session. The response contains the generated stack shape plus setup compatibility metadata.

## getInfo

Get deployment information for the deployment portal. Accepts both deployment-scoped and deployment-group-scoped API keys. Returns project information, package status/outputs, and either deployment or deployment group details depending on the token type. Poll this endpoint to check if packages are ready.

### Example Usage

<!-- UsageSnippet language="typescript" operationID="getDeploymentInfo" method="get" path="/v1/deployment-info" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.deployment.getInfo();

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { deploymentGetInfo } from "@alienplatform/platform-api/funcs/deploymentGetInfo.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await deploymentGetInfo(alien);
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("deploymentGetInfo failed:", res.error);
  }
}

run();
```

### React hooks and utilities

This method can be used in React components through the following hooks and
associated utilities.

> Check out [this guide][hook-guide] for information about each of the utilities
> below and how to get started using React hooks.

[hook-guide]: ../../../REACT_QUERY.md

```tsx
import {
  // Query hooks for fetching data.
  useDeploymentGetInfo,
  useDeploymentGetInfoSuspense,

  // Utility for prefetching data during server-side rendering and in React
  // Server Components that will be immediately available to client components
  // using the hooks.
  prefetchDeploymentGetInfo,

  // Utilities to invalidate the query cache for this query in response to
  // mutations and other user actions.
  invalidateDeploymentGetInfo,
  invalidateAllDeploymentGetInfo,
} from "@alienplatform/platform-api/react-query/deploymentGetInfo.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.GetDeploymentInfoRequest](../../models/operations/getdeploymentinforequest.md)                                                                                     | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.DeploymentInfo](../../models/deploymentinfo.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 400, 401, 403, 404, 409  | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## planCompute

Plan deployment compute for the active release before stack preparation. The response contains recommended machine and scale choices for cloud compute pools.

### Example Usage

<!-- UsageSnippet language="typescript" operationID="planDeploymentCompute" method="post" path="/v1/deployment-info/compute-plan" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.deployment.planCompute();

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { deploymentPlanCompute } from "@alienplatform/platform-api/funcs/deploymentPlanCompute.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await deploymentPlanCompute(alien);
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("deploymentPlanCompute failed:", res.error);
  }
}

run();
```

### React hooks and utilities

This method can be used in React components through the following hooks and
associated utilities.

> Check out [this guide][hook-guide] for information about each of the utilities
> below and how to get started using React hooks.

[hook-guide]: ../../../REACT_QUERY.md

```tsx
import {
  // Mutation hook for triggering the API call.
  useDeploymentPlanComputeMutation
} from "@alienplatform/platform-api/react-query/deploymentPlanCompute.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.PlanDeploymentComputeRequest](../../models/operations/plandeploymentcomputerequest.md)                                                                             | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.DeploymentComputePlan](../../models/deploymentcomputeplan.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 400, 401, 403, 404, 409  | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## prepareStack

Prepare the active release stack for a deployment portal setup session. The response contains the generated stack shape plus setup compatibility metadata.

### Example Usage

<!-- UsageSnippet language="typescript" operationID="prepareDeploymentStack" method="post" path="/v1/deployment-info/prepare-stack" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.deployment.prepareStack();

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { deploymentPrepareStack } from "@alienplatform/platform-api/funcs/deploymentPrepareStack.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await deploymentPrepareStack(alien);
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("deploymentPrepareStack failed:", res.error);
  }
}

run();
```

### React hooks and utilities

This method can be used in React components through the following hooks and
associated utilities.

> Check out [this guide][hook-guide] for information about each of the utilities
> below and how to get started using React hooks.

[hook-guide]: ../../../REACT_QUERY.md

```tsx
import {
  // Mutation hook for triggering the API call.
  useDeploymentPrepareStackMutation
} from "@alienplatform/platform-api/react-query/deploymentPrepareStack.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.PrepareDeploymentStackRequest](../../models/operations/preparedeploymentstackrequest.md)                                                                           | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.PreparedDeploymentStack](../../models/prepareddeploymentstack.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 400, 401, 403, 404, 409  | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |