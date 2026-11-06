# Identity

## Overview

Authentication identity

### Available Operations

* [exchangeRuntimeIdentity](#exchangeruntimeidentity) - Exchange a verified component proof for one bounded runtime purpose.

## exchangeRuntimeIdentity

Exchange a verified component proof for one bounded runtime purpose.

### Example Usage

<!-- UsageSnippet language="typescript" operationID="exchangeRuntimeIdentity" method="post" path="/v1/identity/exchange" -->
```typescript
import { AlienManager } from "@alienplatform/manager-api";

const alienManager = new AlienManager({
  serverURL: "https://api.example.com",
  bearer: process.env["ALIEN_MANAGER_BEARER"] ?? "",
});

async function run() {
  const result = await alienManager.identity.exchangeRuntimeIdentity({
    deploymentId: "<id>",
    proof: {
      token: "<value>",
      type: "session",
    },
    purpose: {
      target: {
        resourceId: "<id>",
        resourceType: "daemon",
      },
      type: "receiveCommands",
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
import { identityExchangeRuntimeIdentity } from "@alienplatform/manager-api/funcs/identityExchangeRuntimeIdentity.js";

// Use `AlienManagerCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alienManager = new AlienManagerCore({
  serverURL: "https://api.example.com",
  bearer: process.env["ALIEN_MANAGER_BEARER"] ?? "",
});

async function run() {
  const res = await identityExchangeRuntimeIdentity(alienManager, {
    deploymentId: "<id>",
    proof: {
      token: "<value>",
      type: "session",
    },
    purpose: {
      target: {
        resourceId: "<id>",
        resourceType: "daemon",
      },
      type: "receiveCommands",
    },
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("identityExchangeRuntimeIdentity failed:", res.error);
  }
}

run();
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [models.IdentityExchangeRequest](../../models/identityexchangerequest.md)                                                                                                      | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.IssuedRuntimeCredential](../../models/issuedruntimecredential.md)\>**

### Errors

| Error Type                      | Status Code                     | Content Type                    |
| ------------------------------- | ------------------------------- | ------------------------------- |
| errors.AlienManagerDefaultError | 4XX, 5XX                        | \*/\*                           |
