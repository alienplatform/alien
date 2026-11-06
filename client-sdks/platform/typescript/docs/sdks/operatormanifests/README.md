# OperatorManifests

## Overview

### Available Operations

* [prepareOperatorManifestPackage](#prepareoperatormanifestpackage) - Prepare the white-labeled Operator image for an Operate install
* [renderOperatorManifest](#renderoperatormanifest) - Render a Kubernetes Operator manifest
* [checkEcsBootstrapWrite](#checkecsbootstrapwrite) - Checks a Remote Operator setup token and its exact AWS target before the local bootstrap command writes a registration secret. A registered stack cannot use this path.
* [renderOperatorEcsCloudFormation](#renderoperatorecscloudformation) - Render a Remote Operator ECS Fargate CloudFormation installer

## prepareOperatorManifestPackage

Prepare the white-labeled Operator image for an Operate install

### Example Usage

<!-- UsageSnippet language="typescript" operationID="prepareOperatorManifestPackage" method="post" path="/v1/operator-manifests/prepare" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.operatorManifests.prepareOperatorManifestPackage({
    project: "my-project",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { operatorManifestsPrepareOperatorManifestPackage } from "@alienplatform/platform-api/funcs/operatorManifestsPrepareOperatorManifestPackage.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await operatorManifestsPrepareOperatorManifestPackage(alien, {
    project: "my-project",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("operatorManifestsPrepareOperatorManifestPackage failed:", res.error);
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
  useOperatorManifestsPrepareOperatorManifestPackageMutation
} from "@alienplatform/platform-api/react-query/operatorManifestsPrepareOperatorManifestPackage.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `project`                                                                                                                                                                      | *string*                                                                                                                                                                       | :heavy_check_mark:                                                                                                                                                             | Filter by project ID or name.                                                                                                                                                  |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.PrepareOperatorManifestPackageResponse](../../models/prepareoperatormanifestpackageresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 404                      | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## renderOperatorManifest

Render a Kubernetes Operator manifest

### Example Usage

<!-- UsageSnippet language="typescript" operationID="renderOperatorManifest" method="post" path="/v1/operator-manifests/render" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.operatorManifests.renderOperatorManifest({
    project: "my-project",
    environmentName: "my-app",
    operatorImagePackageId: "pkg_jebo2o5jmm7raefl2m1pe3cz",
    deploymentGroupToken: "<value>",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { operatorManifestsRenderOperatorManifest } from "@alienplatform/platform-api/funcs/operatorManifestsRenderOperatorManifest.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await operatorManifestsRenderOperatorManifest(alien, {
    project: "my-project",
    environmentName: "my-app",
    operatorImagePackageId: "pkg_jebo2o5jmm7raefl2m1pe3cz",
    deploymentGroupToken: "<value>",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("operatorManifestsRenderOperatorManifest failed:", res.error);
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
  useOperatorManifestsRenderOperatorManifestMutation
} from "@alienplatform/platform-api/react-query/operatorManifestsRenderOperatorManifest.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [models.RenderOperatorManifestRequest](../../models/renderoperatormanifestrequest.md)                                                                                          | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.RenderOperatorManifestResponse](../../models/renderoperatormanifestresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 400, 404, 409            | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## checkEcsBootstrapWrite

Checks a Remote Operator setup token and its exact AWS target before the local bootstrap command writes a registration secret. A registered stack cannot use this path.

### Example Usage

<!-- UsageSnippet language="typescript" operationID="checkEcsBootstrapWrite" method="post" path="/v1/operator-manifests/ecs-cloudformation/bootstrap-write-check" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  await alien.operatorManifests.checkEcsBootstrapWrite({
    stackName: "<value>",
    accountId: "<id>",
    region: "<value>",
    clusterArn: "<value>",
  });


}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { operatorManifestsCheckEcsBootstrapWrite } from "@alienplatform/platform-api/funcs/operatorManifestsCheckEcsBootstrapWrite.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await operatorManifestsCheckEcsBootstrapWrite(alien, {
    stackName: "<value>",
    accountId: "<id>",
    region: "<value>",
    clusterArn: "<value>",
  });
  if (res.ok) {
    const { value: result } = res;
    
  } else {
    console.log("operatorManifestsCheckEcsBootstrapWrite failed:", res.error);
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
  useOperatorManifestsCheckEcsBootstrapWriteMutation
} from "@alienplatform/platform-api/react-query/operatorManifestsCheckEcsBootstrapWrite.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [models.EcsBootstrapWriteCheckRequest](../../models/ecsbootstrapwritecheckrequest.md)                                                                                          | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<void\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 400, 401, 403, 409       | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## renderOperatorEcsCloudFormation

Renders a credential-free CloudFormation artifact that reuses a customer-owned ECS cluster and network; creates retained EFS identity storage unless an existing filesystem and access point are supplied; owns a task role compiled from every built-in operation plugin; and uses a generated local command to hand one-time setup material directly to same-account, same-Region Secrets Manager. Sensitive S3 and SQS wildcard declarations fail closed unless the request supplies exact resource ceilings.

### Example Usage

<!-- UsageSnippet language="typescript" operationID="renderOperatorEcsCloudFormation" method="post" path="/v1/operator-manifests/ecs-cloudformation/render" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.operatorManifests.renderOperatorEcsCloudFormation({
    project: "<value>",
    environmentName: "<value>",
    accountId: "<id>",
    region: "<value>",
    clusterArn: "<value>",
    subnetIds: [
      "<value 1>",
      "<value 2>",
    ],
    securityGroupIds: [],
    operatorImagePackageId: "pkg_jebo2o5jmm7raefl2m1pe3cz",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { operatorManifestsRenderOperatorEcsCloudFormation } from "@alienplatform/platform-api/funcs/operatorManifestsRenderOperatorEcsCloudFormation.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await operatorManifestsRenderOperatorEcsCloudFormation(alien, {
    project: "<value>",
    environmentName: "<value>",
    accountId: "<id>",
    region: "<value>",
    clusterArn: "<value>",
    subnetIds: [
      "<value 1>",
      "<value 2>",
    ],
    securityGroupIds: [],
    operatorImagePackageId: "pkg_jebo2o5jmm7raefl2m1pe3cz",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("operatorManifestsRenderOperatorEcsCloudFormation failed:", res.error);
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
  useOperatorManifestsRenderOperatorEcsCloudFormationMutation
} from "@alienplatform/platform-api/react-query/operatorManifestsRenderOperatorEcsCloudFormation.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [models.RenderOperatorEcsCloudFormationRequest](../../models/renderoperatorecscloudformationrequest.md)                                                                        | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.RenderOperatorEcsCloudFormationResponse](../../models/renderoperatorecscloudformationresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 400, 403, 404, 409, 422  | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |