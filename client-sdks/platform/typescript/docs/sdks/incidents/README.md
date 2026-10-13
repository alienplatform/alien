# Incidents

## Overview

### Available Operations

* [list](#list) - List authorized incidents without Slack transport metadata.
* [get](#get) - Get an authorized incident without Slack transport metadata.

## list

List authorized incidents without Slack transport metadata.

### Example Usage

<!-- UsageSnippet language="typescript" operationID="listIncidents" method="get" path="/v1/incidents" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.incidents.list({});

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { incidentsList } from "@alienplatform/platform-api/funcs/incidentsList.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await incidentsList(alien, {});
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("incidentsList failed:", res.error);
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
  useIncidentsList,
  useIncidentsListSuspense,

  // Utility for prefetching data during server-side rendering and in React
  // Server Components that will be immediately available to client components
  // using the hooks.
  prefetchIncidentsList,

  // Utilities to invalidate the query cache for this query in response to
  // mutations and other user actions.
  invalidateIncidentsList,
  invalidateAllIncidentsList,
} from "@alienplatform/platform-api/react-query/incidentsList.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.ListIncidentsRequest](../../models/operations/listincidentsrequest.md)                                                                                             | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.IncidentListResponse](../../models/incidentlistresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 400, 403, 422            | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## get

Get an authorized incident without Slack transport metadata.

### Example Usage

<!-- UsageSnippet language="typescript" operationID="getIncident" method="get" path="/v1/incidents/{id}" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.incidents.get({
    id: "<id>",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { incidentsGet } from "@alienplatform/platform-api/funcs/incidentsGet.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await incidentsGet(alien, {
    id: "<id>",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("incidentsGet failed:", res.error);
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
  useIncidentsGet,
  useIncidentsGetSuspense,

  // Utility for prefetching data during server-side rendering and in React
  // Server Components that will be immediately available to client components
  // using the hooks.
  prefetchIncidentsGet,

  // Utilities to invalidate the query cache for this query in response to
  // mutations and other user actions.
  invalidateIncidentsGet,
  invalidateAllIncidentsGet,
} from "@alienplatform/platform-api/react-query/incidentsGet.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.GetIncidentRequest](../../models/operations/getincidentrequest.md)                                                                                                 | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.IncidentResponse](../../models/incidentresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 400, 403, 404, 422       | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |