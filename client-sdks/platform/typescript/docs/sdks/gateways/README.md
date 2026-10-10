# Gateways

## Overview

### Available Operations

* [getWorkspaceOverview](#getworkspaceoverview) - Get compact cross-Project setup and customer status for a workspace Gateway.

## getWorkspaceOverview

Get compact cross-Project setup and customer status for a workspace Gateway.

### Example Usage

<!-- UsageSnippet language="typescript" operationID="getWorkspaceGatewayOverview" method="get" path="/v1/gateways/{gateway}/overview" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.gateways.getWorkspaceOverview({
    gateway: "ai",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { gatewaysGetWorkspaceOverview } from "@alienplatform/platform-api/funcs/gatewaysGetWorkspaceOverview.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await gatewaysGetWorkspaceOverview(alien, {
    gateway: "ai",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("gatewaysGetWorkspaceOverview failed:", res.error);
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
  useGatewaysGetWorkspaceOverview,
  useGatewaysGetWorkspaceOverviewSuspense,

  // Utility for prefetching data during server-side rendering and in React
  // Server Components that will be immediately available to client components
  // using the hooks.
  prefetchGatewaysGetWorkspaceOverview,
  
  // Utilities to invalidate the query cache for this query in response to
  // mutations and other user actions.
  invalidateGatewaysGetWorkspaceOverview,
  invalidateAllGatewaysGetWorkspaceOverview,
} from "@alienplatform/platform-api/react-query/gatewaysGetWorkspaceOverview.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.GetWorkspaceGatewayOverviewRequest](../../models/operations/getworkspacegatewayoverviewrequest.md)                                                                 | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.WorkspaceGatewayOverview](../../models/workspacegatewayoverview.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 403                      | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |