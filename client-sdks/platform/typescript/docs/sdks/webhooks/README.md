# Webhooks

## Overview

### Available Operations

* [listWebhookEndpoints](#listwebhookendpoints)
* [createWebhookEndpoint](#createwebhookendpoint) - Create an endpoint. Invalid delivery-service configuration returns 400 with the validation reason.
* [getWebhookEndpoint](#getwebhookendpoint)
* [updateWebhookEndpoint](#updatewebhookendpoint)
* [deleteWebhookEndpoint](#deletewebhookendpoint) - Delete an endpoint idempotently. An endpoint that is already absent returns success. Deleting the last workspace endpoint stops capturing new webhook events.
* [getWebhookSigningSecret](#getwebhooksigningsecret)
* [rotateWebhookSigningSecret](#rotatewebhooksigningsecret)
* [listWebhookDeliveries](#listwebhookdeliveries)
* [retryWebhookDelivery](#retrywebhookdelivery)
* [testWebhookEndpoint](#testwebhookendpoint)

## listWebhookEndpoints

### Example Usage

<!-- UsageSnippet language="typescript" operationID="listWebhookEndpoints" method="get" path="/v1/webhooks/endpoints" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.webhooks.listWebhookEndpoints();

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { webhooksListWebhookEndpoints } from "@alienplatform/platform-api/funcs/webhooksListWebhookEndpoints.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await webhooksListWebhookEndpoints(alien);
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("webhooksListWebhookEndpoints failed:", res.error);
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
  useWebhooksListWebhookEndpoints,
  useWebhooksListWebhookEndpointsSuspense,

  // Utility for prefetching data during server-side rendering and in React
  // Server Components that will be immediately available to client components
  // using the hooks.
  prefetchWebhooksListWebhookEndpoints,

  // Utilities to invalidate the query cache for this query in response to
  // mutations and other user actions.
  invalidateWebhooksListWebhookEndpoints,
  invalidateAllWebhooksListWebhookEndpoints,
} from "@alienplatform/platform-api/react-query/webhooksListWebhookEndpoints.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.ListWebhookEndpointsRequest](../../models/operations/listwebhookendpointsrequest.md)                                                                               | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.WebhookEndpointList](../../models/webhookendpointlist.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 400, 403, 404            | application/json         |
| errors.APIError          | 500, 502, 503            | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## createWebhookEndpoint

Create an endpoint. Invalid delivery-service configuration returns 400 with the validation reason.

### Example Usage

<!-- UsageSnippet language="typescript" operationID="createWebhookEndpoint" method="post" path="/v1/webhooks/endpoints" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.webhooks.createWebhookEndpoint({
    url: "https://cautious-litter.net/",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { webhooksCreateWebhookEndpoint } from "@alienplatform/platform-api/funcs/webhooksCreateWebhookEndpoint.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await webhooksCreateWebhookEndpoint(alien, {
    url: "https://cautious-litter.net/",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("webhooksCreateWebhookEndpoint failed:", res.error);
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
  useWebhooksCreateWebhookEndpointMutation
} from "@alienplatform/platform-api/react-query/webhooksCreateWebhookEndpoint.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `requestBody`                                                                                                                                                                  | [operations.CreateWebhookEndpointRequestBody](../../models/operations/createwebhookendpointrequestbody.md)                                                                     | :heavy_check_mark:                                                                                                                                                             | N/A                                                                                                                                                                            |
| `projectId`                                                                                                                                                                    | *string*                                                                                                                                                                       | :heavy_minus_sign:                                                                                                                                                             | N/A                                                                                                                                                                            |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.WebhookEndpoint](../../models/webhookendpoint.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 400, 403, 404            | application/json         |
| errors.APIError          | 500, 502, 503            | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## getWebhookEndpoint

### Example Usage

<!-- UsageSnippet language="typescript" operationID="getWebhookEndpoint" method="get" path="/v1/webhooks/endpoints/{id}" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.webhooks.getWebhookEndpoint({
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
import { webhooksGetWebhookEndpoint } from "@alienplatform/platform-api/funcs/webhooksGetWebhookEndpoint.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await webhooksGetWebhookEndpoint(alien, {
    id: "<id>",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("webhooksGetWebhookEndpoint failed:", res.error);
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
  useWebhooksGetWebhookEndpoint,
  useWebhooksGetWebhookEndpointSuspense,

  // Utility for prefetching data during server-side rendering and in React
  // Server Components that will be immediately available to client components
  // using the hooks.
  prefetchWebhooksGetWebhookEndpoint,

  // Utilities to invalidate the query cache for this query in response to
  // mutations and other user actions.
  invalidateWebhooksGetWebhookEndpoint,
  invalidateAllWebhooksGetWebhookEndpoint,
} from "@alienplatform/platform-api/react-query/webhooksGetWebhookEndpoint.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.GetWebhookEndpointRequest](../../models/operations/getwebhookendpointrequest.md)                                                                                   | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.WebhookEndpoint](../../models/webhookendpoint.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 400, 403, 404            | application/json         |
| errors.APIError          | 500, 502, 503            | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## updateWebhookEndpoint

### Example Usage

<!-- UsageSnippet language="typescript" operationID="updateWebhookEndpoint" method="patch" path="/v1/webhooks/endpoints/{id}" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.webhooks.updateWebhookEndpoint({
    id: "<id>",
    requestBody: {},
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { webhooksUpdateWebhookEndpoint } from "@alienplatform/platform-api/funcs/webhooksUpdateWebhookEndpoint.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await webhooksUpdateWebhookEndpoint(alien, {
    id: "<id>",
    requestBody: {},
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("webhooksUpdateWebhookEndpoint failed:", res.error);
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
  useWebhooksUpdateWebhookEndpointMutation
} from "@alienplatform/platform-api/react-query/webhooksUpdateWebhookEndpoint.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.UpdateWebhookEndpointRequest](../../models/operations/updatewebhookendpointrequest.md)                                                                             | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.WebhookEndpoint](../../models/webhookendpoint.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 400, 403, 404            | application/json         |
| errors.APIError          | 500, 502, 503            | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## deleteWebhookEndpoint

Delete an endpoint idempotently. An endpoint that is already absent returns success. Deleting the last workspace endpoint stops capturing new webhook events.

### Example Usage

<!-- UsageSnippet language="typescript" operationID="deleteWebhookEndpoint" method="delete" path="/v1/webhooks/endpoints/{id}" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.webhooks.deleteWebhookEndpoint({
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
import { webhooksDeleteWebhookEndpoint } from "@alienplatform/platform-api/funcs/webhooksDeleteWebhookEndpoint.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await webhooksDeleteWebhookEndpoint(alien, {
    id: "<id>",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("webhooksDeleteWebhookEndpoint failed:", res.error);
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
  useWebhooksDeleteWebhookEndpointMutation
} from "@alienplatform/platform-api/react-query/webhooksDeleteWebhookEndpoint.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.DeleteWebhookEndpointRequest](../../models/operations/deletewebhookendpointrequest.md)                                                                             | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.WebhookActionResult](../../models/webhookactionresult.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 400, 403, 404            | application/json         |
| errors.APIError          | 500, 502, 503            | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## getWebhookSigningSecret

### Example Usage

<!-- UsageSnippet language="typescript" operationID="getWebhookSigningSecret" method="get" path="/v1/webhooks/endpoints/{id}/secret" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.webhooks.getWebhookSigningSecret({
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
import { webhooksGetWebhookSigningSecret } from "@alienplatform/platform-api/funcs/webhooksGetWebhookSigningSecret.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await webhooksGetWebhookSigningSecret(alien, {
    id: "<id>",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("webhooksGetWebhookSigningSecret failed:", res.error);
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
  useWebhooksGetWebhookSigningSecret,
  useWebhooksGetWebhookSigningSecretSuspense,

  // Utility for prefetching data during server-side rendering and in React
  // Server Components that will be immediately available to client components
  // using the hooks.
  prefetchWebhooksGetWebhookSigningSecret,

  // Utilities to invalidate the query cache for this query in response to
  // mutations and other user actions.
  invalidateWebhooksGetWebhookSigningSecret,
  invalidateAllWebhooksGetWebhookSigningSecret,
} from "@alienplatform/platform-api/react-query/webhooksGetWebhookSigningSecret.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.GetWebhookSigningSecretRequest](../../models/operations/getwebhooksigningsecretrequest.md)                                                                         | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.WebhookSigningSecret](../../models/webhooksigningsecret.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 400, 403, 404            | application/json         |
| errors.APIError          | 500, 502, 503            | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## rotateWebhookSigningSecret

### Example Usage

<!-- UsageSnippet language="typescript" operationID="rotateWebhookSigningSecret" method="post" path="/v1/webhooks/endpoints/{id}/rotate-secret" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.webhooks.rotateWebhookSigningSecret({
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
import { webhooksRotateWebhookSigningSecret } from "@alienplatform/platform-api/funcs/webhooksRotateWebhookSigningSecret.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await webhooksRotateWebhookSigningSecret(alien, {
    id: "<id>",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("webhooksRotateWebhookSigningSecret failed:", res.error);
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
  useWebhooksRotateWebhookSigningSecretMutation
} from "@alienplatform/platform-api/react-query/webhooksRotateWebhookSigningSecret.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.RotateWebhookSigningSecretRequest](../../models/operations/rotatewebhooksigningsecretrequest.md)                                                                   | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.WebhookActionResult](../../models/webhookactionresult.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 400, 403, 404            | application/json         |
| errors.APIError          | 500, 502, 503            | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## listWebhookDeliveries

### Example Usage

<!-- UsageSnippet language="typescript" operationID="listWebhookDeliveries" method="get" path="/v1/webhooks/endpoints/{id}/deliveries" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.webhooks.listWebhookDeliveries({
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
import { webhooksListWebhookDeliveries } from "@alienplatform/platform-api/funcs/webhooksListWebhookDeliveries.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await webhooksListWebhookDeliveries(alien, {
    id: "<id>",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("webhooksListWebhookDeliveries failed:", res.error);
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
  useWebhooksListWebhookDeliveries,
  useWebhooksListWebhookDeliveriesSuspense,

  // Utility for prefetching data during server-side rendering and in React
  // Server Components that will be immediately available to client components
  // using the hooks.
  prefetchWebhooksListWebhookDeliveries,

  // Utilities to invalidate the query cache for this query in response to
  // mutations and other user actions.
  invalidateWebhooksListWebhookDeliveries,
  invalidateAllWebhooksListWebhookDeliveries,
} from "@alienplatform/platform-api/react-query/webhooksListWebhookDeliveries.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.ListWebhookDeliveriesRequest](../../models/operations/listwebhookdeliveriesrequest.md)                                                                             | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.WebhookDeliveryList](../../models/webhookdeliverylist.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 400, 403, 404            | application/json         |
| errors.APIError          | 500, 502, 503            | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## retryWebhookDelivery

### Example Usage

<!-- UsageSnippet language="typescript" operationID="retryWebhookDelivery" method="post" path="/v1/webhooks/endpoints/{id}/retry" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.webhooks.retryWebhookDelivery({
    id: "<id>",
    requestBody: {
      messageId: "<id>",
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
import { webhooksRetryWebhookDelivery } from "@alienplatform/platform-api/funcs/webhooksRetryWebhookDelivery.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await webhooksRetryWebhookDelivery(alien, {
    id: "<id>",
    requestBody: {
      messageId: "<id>",
    },
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("webhooksRetryWebhookDelivery failed:", res.error);
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
  useWebhooksRetryWebhookDeliveryMutation
} from "@alienplatform/platform-api/react-query/webhooksRetryWebhookDelivery.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.RetryWebhookDeliveryRequest](../../models/operations/retrywebhookdeliveryrequest.md)                                                                               | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.WebhookActionResult](../../models/webhookactionresult.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 400, 403, 404            | application/json         |
| errors.APIError          | 500, 502, 503            | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## testWebhookEndpoint

### Example Usage

<!-- UsageSnippet language="typescript" operationID="testWebhookEndpoint" method="post" path="/v1/webhooks/endpoints/{id}/test" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.webhooks.testWebhookEndpoint({
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
import { webhooksTestWebhookEndpoint } from "@alienplatform/platform-api/funcs/webhooksTestWebhookEndpoint.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await webhooksTestWebhookEndpoint(alien, {
    id: "<id>",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("webhooksTestWebhookEndpoint failed:", res.error);
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
  useWebhooksTestWebhookEndpointMutation
} from "@alienplatform/platform-api/react-query/webhooksTestWebhookEndpoint.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.TestWebhookEndpointRequest](../../models/operations/testwebhookendpointrequest.md)                                                                                 | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.WebhookActionResult](../../models/webhookactionresult.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 400, 403, 404            | application/json         |
| errors.APIError          | 500, 502, 503            | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |