# RemoteBindings

## Overview

### Available Operations

* [createExternalAccess](#createexternalaccess) - Create short-lived Remote Bindings access for a external resource

## createExternalAccess

Selects a connected external resource by Project and external ID, then returns a short-lived deployment-scoped Manager capability. The caller never receives the external cloud credentials from Platform.

### Example Usage

<!-- UsageSnippet language="typescript" operationID="createRemoteBindingsExternalAccess" method="post" path="/v1/projects/{idOrName}/remote-bindings/access" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.remoteBindings.createExternalAccess({
    idOrName: "my-project",
    workspace: "my-workspace",
    remoteBindingsExternalAccessRequest: {
      externalId: "ext_example_01",
      capability: "storage",
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
import { remoteBindingsCreateExternalAccess } from "@alienplatform/platform-api/funcs/remoteBindingsCreateExternalAccess.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await remoteBindingsCreateExternalAccess(alien, {
    idOrName: "my-project",
    workspace: "my-workspace",
    remoteBindingsExternalAccessRequest: {
      externalId: "ext_example_01",
      capability: "storage",
    },
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("remoteBindingsCreateExternalAccess failed:", res.error);
  }
}

run();
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.CreateRemoteBindingsExternalAccessRequest](../../models/operations/createremotebindingsexternalaccessrequest.md)                                                   | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.RemoteBindingsExternalAccessResponse](../../models/remotebindingsexternalaccessresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 401, 403, 404, 409       | application/json         |
| errors.APIError          | 500, 503                 | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |