# ContainerRegistry

## Overview

### Available Operations

* [getContainerRegistry](#getcontainerregistry)
* [putContainerRegistry](#putcontainerregistry)
* [createContainerRegistryRepository](#createcontainerregistryrepository)
* [createContainerRegistryCredential](#createcontainerregistrycredential)
* [revokeContainerRegistryCredential](#revokecontainerregistrycredential)
* [deleteContainerRegistryRepository](#deletecontainerregistryrepository)
* [verifyContainerRegistry](#verifycontainerregistry)
* [revokeContainerRegistry](#revokecontainerregistry)
* [getContainerRegistryManagerSnapshot](#getcontainerregistrymanagersnapshot)
* [applyContainerRegistryManagerSnapshot](#applycontainerregistrymanagersnapshot)

## getContainerRegistry

### Example Usage

<!-- UsageSnippet language="typescript" operationID="getContainerRegistry" method="get" path="/v1/deployment-groups/{id}/container-registry" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.containerRegistry.getContainerRegistry({
    id: "dg_r27ict8c7vcgsumpj90ackf7b",
    workspace: "my-workspace",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { containerRegistryGetContainerRegistry } from "@alienplatform/platform-api/funcs/containerRegistryGetContainerRegistry.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await containerRegistryGetContainerRegistry(alien, {
    id: "dg_r27ict8c7vcgsumpj90ackf7b",
    workspace: "my-workspace",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("containerRegistryGetContainerRegistry failed:", res.error);
  }
}

run();
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.GetContainerRegistryRequest](../../models/operations/getcontainerregistryrequest.md)                                                                               | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.ContainerRegistryState](../../models/containerregistrystate.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 404, 409                 | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## putContainerRegistry

### Example Usage

<!-- UsageSnippet language="typescript" operationID="putContainerRegistry" method="put" path="/v1/deployment-groups/{id}/container-registry" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.containerRegistry.putContainerRegistry({
    id: "dg_r27ict8c7vcgsumpj90ackf7b",
    workspace: "my-workspace",
    requestBody: {
      deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
      resourceId: "<id>",
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
import { containerRegistryPutContainerRegistry } from "@alienplatform/platform-api/funcs/containerRegistryPutContainerRegistry.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await containerRegistryPutContainerRegistry(alien, {
    id: "dg_r27ict8c7vcgsumpj90ackf7b",
    workspace: "my-workspace",
    requestBody: {
      deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
      resourceId: "<id>",
    },
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("containerRegistryPutContainerRegistry failed:", res.error);
  }
}

run();
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.PutContainerRegistryRequest](../../models/operations/putcontainerregistryrequest.md)                                                                               | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[operations.PutContainerRegistryResponse](../../models/operations/putcontainerregistryresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 404, 409                 | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## createContainerRegistryRepository

### Example Usage

<!-- UsageSnippet language="typescript" operationID="createContainerRegistryRepository" method="post" path="/v1/deployment-groups/{id}/container-registry/repositories" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.containerRegistry.createContainerRegistryRepository({
    id: "dg_r27ict8c7vcgsumpj90ackf7b",
    workspace: "my-workspace",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { containerRegistryCreateContainerRegistryRepository } from "@alienplatform/platform-api/funcs/containerRegistryCreateContainerRegistryRepository.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await containerRegistryCreateContainerRegistryRepository(alien, {
    id: "dg_r27ict8c7vcgsumpj90ackf7b",
    workspace: "my-workspace",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("containerRegistryCreateContainerRegistryRepository failed:", res.error);
  }
}

run();
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.CreateContainerRegistryRepositoryRequest](../../models/operations/createcontainerregistryrepositoryrequest.md)                                                     | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[operations.CreateContainerRegistryRepositoryResponse](../../models/operations/createcontainerregistryrepositoryresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 404, 409                 | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## createContainerRegistryCredential

### Example Usage

<!-- UsageSnippet language="typescript" operationID="createContainerRegistryCredential" method="post" path="/v1/deployment-groups/{id}/container-registry/credentials" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.containerRegistry.createContainerRegistryCredential({
    id: "dg_r27ict8c7vcgsumpj90ackf7b",
    workspace: "my-workspace",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { containerRegistryCreateContainerRegistryCredential } from "@alienplatform/platform-api/funcs/containerRegistryCreateContainerRegistryCredential.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await containerRegistryCreateContainerRegistryCredential(alien, {
    id: "dg_r27ict8c7vcgsumpj90ackf7b",
    workspace: "my-workspace",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("containerRegistryCreateContainerRegistryCredential failed:", res.error);
  }
}

run();
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.CreateContainerRegistryCredentialRequest](../../models/operations/createcontainerregistrycredentialrequest.md)                                                     | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.CreateContainerRegistryCredentialResponse](../../models/createcontainerregistrycredentialresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 404, 409                 | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## revokeContainerRegistryCredential

### Example Usage

<!-- UsageSnippet language="typescript" operationID="revokeContainerRegistryCredential" method="delete" path="/v1/deployment-groups/{id}/container-registry/credentials/{credentialId}" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  await alien.containerRegistry.revokeContainerRegistryCredential({
    id: "dg_r27ict8c7vcgsumpj90ackf7b",
    credentialId: "crcred_oz1xjr82f37j17g4gtmyu",
    workspace: "my-workspace",
  });


}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { containerRegistryRevokeContainerRegistryCredential } from "@alienplatform/platform-api/funcs/containerRegistryRevokeContainerRegistryCredential.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await containerRegistryRevokeContainerRegistryCredential(alien, {
    id: "dg_r27ict8c7vcgsumpj90ackf7b",
    credentialId: "crcred_oz1xjr82f37j17g4gtmyu",
    workspace: "my-workspace",
  });
  if (res.ok) {
    const { value: result } = res;
    
  } else {
    console.log("containerRegistryRevokeContainerRegistryCredential failed:", res.error);
  }
}

run();
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.RevokeContainerRegistryCredentialRequest](../../models/operations/revokecontainerregistrycredentialrequest.md)                                                     | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<void\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 404, 409                 | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## deleteContainerRegistryRepository

### Example Usage

<!-- UsageSnippet language="typescript" operationID="deleteContainerRegistryRepository" method="delete" path="/v1/deployment-groups/{id}/container-registry/repositories/{repositoryId}" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.containerRegistry.deleteContainerRegistryRepository({
    id: "dg_r27ict8c7vcgsumpj90ackf7b",
    repositoryId: "crrepo_625temdq3bnu25jw9rcux",
    workspace: "my-workspace",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { containerRegistryDeleteContainerRegistryRepository } from "@alienplatform/platform-api/funcs/containerRegistryDeleteContainerRegistryRepository.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await containerRegistryDeleteContainerRegistryRepository(alien, {
    id: "dg_r27ict8c7vcgsumpj90ackf7b",
    repositoryId: "crrepo_625temdq3bnu25jw9rcux",
    workspace: "my-workspace",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("containerRegistryDeleteContainerRegistryRepository failed:", res.error);
  }
}

run();
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.DeleteContainerRegistryRepositoryRequest](../../models/operations/deletecontainerregistryrepositoryrequest.md)                                                     | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[operations.DeleteContainerRegistryRepositoryResponse](../../models/operations/deletecontainerregistryrepositoryresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 404, 409                 | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## verifyContainerRegistry

### Example Usage

<!-- UsageSnippet language="typescript" operationID="verifyContainerRegistry" method="post" path="/v1/deployment-groups/{id}/container-registry/verify" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.containerRegistry.verifyContainerRegistry({
    id: "dg_r27ict8c7vcgsumpj90ackf7b",
    workspace: "my-workspace",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { containerRegistryVerifyContainerRegistry } from "@alienplatform/platform-api/funcs/containerRegistryVerifyContainerRegistry.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await containerRegistryVerifyContainerRegistry(alien, {
    id: "dg_r27ict8c7vcgsumpj90ackf7b",
    workspace: "my-workspace",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("containerRegistryVerifyContainerRegistry failed:", res.error);
  }
}

run();
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.VerifyContainerRegistryRequest](../../models/operations/verifycontainerregistryrequest.md)                                                                         | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[operations.VerifyContainerRegistryResponse](../../models/operations/verifycontainerregistryresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 404, 409                 | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## revokeContainerRegistry

### Example Usage

<!-- UsageSnippet language="typescript" operationID="revokeContainerRegistry" method="post" path="/v1/deployment-groups/{id}/container-registry/revoke" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.containerRegistry.revokeContainerRegistry({
    id: "dg_r27ict8c7vcgsumpj90ackf7b",
    workspace: "my-workspace",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { containerRegistryRevokeContainerRegistry } from "@alienplatform/platform-api/funcs/containerRegistryRevokeContainerRegistry.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await containerRegistryRevokeContainerRegistry(alien, {
    id: "dg_r27ict8c7vcgsumpj90ackf7b",
    workspace: "my-workspace",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("containerRegistryRevokeContainerRegistry failed:", res.error);
  }
}

run();
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.RevokeContainerRegistryRequest](../../models/operations/revokecontainerregistryrequest.md)                                                                         | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[operations.RevokeContainerRegistryResponse](../../models/operations/revokecontainerregistryresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 404, 409                 | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## getContainerRegistryManagerSnapshot

### Example Usage

<!-- UsageSnippet language="typescript" operationID="getContainerRegistryManagerSnapshot" method="get" path="/v1/managers/{id}/container-registry/snapshot" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.containerRegistry.getContainerRegistryManagerSnapshot({
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
import { containerRegistryGetContainerRegistryManagerSnapshot } from "@alienplatform/platform-api/funcs/containerRegistryGetContainerRegistryManagerSnapshot.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await containerRegistryGetContainerRegistryManagerSnapshot(alien, {
    id: "<id>",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("containerRegistryGetContainerRegistryManagerSnapshot failed:", res.error);
  }
}

run();
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.GetContainerRegistryManagerSnapshotRequest](../../models/operations/getcontainerregistrymanagersnapshotrequest.md)                                                 | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.ContainerRegistryManagerSnapshot](../../models/containerregistrymanagersnapshot.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 403, 409                 | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## applyContainerRegistryManagerSnapshot

### Example Usage

<!-- UsageSnippet language="typescript" operationID="applyContainerRegistryManagerSnapshot" method="post" path="/v1/managers/{id}/container-registry/applied" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.containerRegistry.applyContainerRegistryManagerSnapshot({
    id: "<id>",
    requestBody: {
      routeId: "crroute_c9t4xoy7fmiq7equtu20",
      desiredRevision: 186433,
      repositories: [
        {
          id: "crrepo_625temdq3bnu25jw9rcux",
          status: "failed",
          routableUpstreamName: "<value>",
          error: "<value>",
        },
      ],
      verification: {
        succeeded: false,
        observedAt: new Date("2026-11-26T10:14:03.486Z"),
        error: "<value>",
      },
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
import { containerRegistryApplyContainerRegistryManagerSnapshot } from "@alienplatform/platform-api/funcs/containerRegistryApplyContainerRegistryManagerSnapshot.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await containerRegistryApplyContainerRegistryManagerSnapshot(alien, {
    id: "<id>",
    requestBody: {
      routeId: "crroute_c9t4xoy7fmiq7equtu20",
      desiredRevision: 186433,
      repositories: [
        {
          id: "crrepo_625temdq3bnu25jw9rcux",
          status: "failed",
          routableUpstreamName: "<value>",
          error: "<value>",
        },
      ],
      verification: {
        succeeded: false,
        observedAt: new Date("2026-11-26T10:14:03.486Z"),
        error: "<value>",
      },
    },
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("containerRegistryApplyContainerRegistryManagerSnapshot failed:", res.error);
  }
}

run();
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.ApplyContainerRegistryManagerSnapshotRequest](../../models/operations/applycontainerregistrymanagersnapshotrequest.md)                                             | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.ApplyContainerRegistrySnapshotResponse](../../models/applycontainerregistrysnapshotresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 403, 409                 | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |