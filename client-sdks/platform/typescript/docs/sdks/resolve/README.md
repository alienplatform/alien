# Resolve

## Overview

### Available Operations

* [resolve](#resolve) - Resolve manager for a project and platform

## resolve

Returns the manager URL for a given project and platform. The project can be provided as a query parameter, or derived from the token's scope (project-scoped, deployment-group-scoped, or deployment-scoped tokens carry an implicit project). This is the single entry point for all CLI tools to discover which manager to talk to.

### Example Usage

<!-- UsageSnippet language="typescript" operationID="resolve" method="get" path="/v1/resolve" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.resolve.resolve({
    workspace: "my-workspace",
    platform: "test",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { resolveResolve } from "@alienplatform/platform-api/funcs/resolveResolve.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await resolveResolve(alien, {
    workspace: "my-workspace",
    platform: "test",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("resolveResolve failed:", res.error);
  }
}

run();
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.ResolveRequest](../../models/operations/resolverequest.md)                                                                                                         | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.ResolveResponse](../../models/resolveresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 400, 404                 | application/json         |
| errors.APIError          | 503                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |