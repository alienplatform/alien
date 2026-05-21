# StackImport

## Overview

Setup artifact stack import (CFN, TF, Helm)

### Available Operations

* [stackImport](#stackimport) - `POST /v1/stack/import` — Inbound: deployment-group bearer.

## stackImport

The body's `deploymentGroupToken` field is informational (mirrored back
for log correlation) — actual authentication is the standard `Authorization
Bearer` header processed by [`auth::require_auth`]. The handler tolerates
the body field being either the raw token or empty; it never reads
credentials from the body to make the secret path uniform with every
other endpoint.

### Example Usage

<!-- UsageSnippet language="typescript" operationID="stack_import" method="post" path="/v1/stack/import" -->
```typescript
import { AlienManager } from "@alienplatform/manager-api";

const alienManager = new AlienManager({
  serverURL: "https://api.example.com",
  bearer: process.env["ALIEN_MANAGER_BEARER"] ?? "",
});

async function run() {
  const result = await alienManager.stackImport.stackImport({
    deploymentGroupToken: "<value>",
    deploymentName: "<value>",
    managementConfig: {
      serviceAccountEmail: "<value>",
      platform: "gcp",
    },
    platform: "gcp",
    region: "<value>",
    resourcePrefix: "<value>",
    resources: [],
    setupFingerprint: "<value>",
    setupFingerprintVersion: 325467,
    setupTarget: "<value>",
    stackSettings: {},
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienManagerCore } from "@alienplatform/manager-api/core.js";
import { stackImportStackImport } from "@alienplatform/manager-api/funcs/stackImportStackImport.js";

// Use `AlienManagerCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alienManager = new AlienManagerCore({
  serverURL: "https://api.example.com",
  bearer: process.env["ALIEN_MANAGER_BEARER"] ?? "",
});

async function run() {
  const res = await stackImportStackImport(alienManager, {
    deploymentGroupToken: "<value>",
    deploymentName: "<value>",
    managementConfig: {
      serviceAccountEmail: "<value>",
      platform: "gcp",
    },
    platform: "gcp",
    region: "<value>",
    resourcePrefix: "<value>",
    resources: [],
    setupFingerprint: "<value>",
    setupFingerprintVersion: 325467,
    setupTarget: "<value>",
    stackSettings: {},
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("stackImportStackImport failed:", res.error);
  }
}

run();
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [models.StackImportRequest](../../models/stackimportrequest.md)                                                                                                                | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.StackImportResponse](../../models/stackimportresponse.md)\>**

### Errors

| Error Type                      | Status Code                     | Content Type                    |
| ------------------------------- | ------------------------------- | ------------------------------- |
| errors.AlienManagerDefaultError | 4XX, 5XX                        | \*/\*                           |