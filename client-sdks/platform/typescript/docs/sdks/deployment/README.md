# Deployment

## Overview

### Available Operations

* [getInfo](#getinfo) - Get deployment information for the deployment page. Accepts both agent-scoped and deployment-group-scoped API keys. Returns project information, package status/outputs, and either agent or deployment group details depending on the token type. Poll this endpoint to check if packages are ready.

## getInfo

Get deployment information for the deployment page. Accepts both agent-scoped and deployment-group-scoped API keys. Returns project information, package status/outputs, and either agent or deployment group details depending on the token type. Poll this endpoint to check if packages are ready.

### Example Usage

<!-- UsageSnippet language="typescript" operationID="getDeploymentInfo" method="get" path="/v1/deployment-info" -->
```typescript
import { Alien } from "@aliendotdev/platform-api";

const alien = new Alien({
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.deployment.getInfo({
    workspace: "my-workspace",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@aliendotdev/platform-api/core.js";
import { deploymentGetInfo } from "@aliendotdev/platform-api/funcs/deploymentGetInfo.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await deploymentGetInfo(alien, {
    workspace: "my-workspace",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("deploymentGetInfo failed:", res.error);
  }
}

run();
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
| errors.APIError          | 401, 404                 | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |