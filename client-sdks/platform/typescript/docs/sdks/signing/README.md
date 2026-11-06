# Signing

## Overview

### Available Operations

* [listSignerLists](#listsignerlists) - Every stored version of the project's signer list, oldest first, exactly as signed. Readable by whoever may read the project's releases.
* [addSignerList](#addsignerlist) - Store the next version of the project's signer list. The version is the statement's sequence and must follow the last stored one. A stored version never changes. Storing version 1 turns signing on; a second, different version 1 is refused with who turned it on.
* [getProjectSigning](#getprojectsigning) - The project's signing as the dashboard and `alien devices list` show it: fingerprint, trusted devices, recovery and waiting changes. Decoded from the stored lists for display; nothing here is verified.
* [listPendingChanges](#listpendingchanges) - The project's prepared signer list changes with the given status, newest first.
* [createPendingChange](#creatependingchange) - File the body of the next signer list for an admin to sign with `alien approve`: a device asking to be trusted, a CI key, a removal or a refresh. The body is stored as received; the CLI shows the admin a diff against the list it verified.
* [getPendingChange](#getpendingchange)
* [resolvePendingChange](#resolvependingchange) - Resolve a pending change: approved, naming the stored signer list version that carries it, or rejected. The platform checks only that the version is stored.
* [addDeploymentConfigStatement](#adddeploymentconfigstatement) - Store a signed `config/v1` statement for the deployment: developer environment variables and stack inputs, signed by a trusted device or CI. The newest sequence is delivered on sync.
* [listDeploymentConfigStatements](#listdeploymentconfigstatements) - Every signed `config/v1` statement stored for the deployment, lowest sequence first, exactly as stored. The CLI builds the next statement on the newest and checks the chain when it rotates the CI key.
* [getLatestDeploymentConfigStatement](#getlatestdeploymentconfigstatement) - The deployment's newest signed `config/v1` statement, exactly as stored. The manager delivers it to enforcers on sync.

## listSignerLists

Every stored version of the project's signer list, oldest first, exactly as signed. Readable by whoever may read the project's releases.

### Example Usage: projectId

<!-- UsageSnippet language="typescript" operationID="listSignerLists" method="get" path="/v1/projects/{idOrName}/signer-lists" example="projectId" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.signing.listSignerLists({
    idOrName: "prj_mcytp6z3j91f7tn5ryqsfwtrdqkv",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { signingListSignerLists } from "@alienplatform/platform-api/funcs/signingListSignerLists.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await signingListSignerLists(alien, {
    idOrName: "prj_mcytp6z3j91f7tn5ryqsfwtrdqkv",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("signingListSignerLists failed:", res.error);
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
  useSigningListSignerLists,
  useSigningListSignerListsSuspense,

  // Utility for prefetching data during server-side rendering and in React
  // Server Components that will be immediately available to client components
  // using the hooks.
  prefetchSigningListSignerLists,

  // Utilities to invalidate the query cache for this query in response to
  // mutations and other user actions.
  invalidateSigningListSignerLists,
  invalidateAllSigningListSignerLists,
} from "@alienplatform/platform-api/react-query/signingListSignerLists.js";
```
### Example Usage: projectName

<!-- UsageSnippet language="typescript" operationID="listSignerLists" method="get" path="/v1/projects/{idOrName}/signer-lists" example="projectName" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.signing.listSignerLists({
    idOrName: "my-project",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { signingListSignerLists } from "@alienplatform/platform-api/funcs/signingListSignerLists.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await signingListSignerLists(alien, {
    idOrName: "my-project",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("signingListSignerLists failed:", res.error);
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
  useSigningListSignerLists,
  useSigningListSignerListsSuspense,

  // Utility for prefetching data during server-side rendering and in React
  // Server Components that will be immediately available to client components
  // using the hooks.
  prefetchSigningListSignerLists,

  // Utilities to invalidate the query cache for this query in response to
  // mutations and other user actions.
  invalidateSigningListSignerLists,
  invalidateAllSigningListSignerLists,
} from "@alienplatform/platform-api/react-query/signingListSignerLists.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.ListSignerListsRequest](../../models/operations/listsignerlistsrequest.md)                                                                                         | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.SignerListsResponse](../../models/signerlistsresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 400, 403, 404            | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## addSignerList

Store the next version of the project's signer list. The version is the statement's sequence and must follow the last stored one. A stored version never changes. Storing version 1 turns signing on; a second, different version 1 is refused with who turned it on.

### Example Usage: projectId

<!-- UsageSnippet language="typescript" operationID="addSignerList" method="post" path="/v1/projects/{idOrName}/signer-lists" example="projectId" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.signing.addSignerList("prj_mcytp6z3j91f7tn5ryqsfwtrdqkv");

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { signingAddSignerList } from "@alienplatform/platform-api/funcs/signingAddSignerList.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await signingAddSignerList(alien, "prj_mcytp6z3j91f7tn5ryqsfwtrdqkv");
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("signingAddSignerList failed:", res.error);
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
  useSigningAddSignerListMutation
} from "@alienplatform/platform-api/react-query/signingAddSignerList.js";
```
### Example Usage: projectName

<!-- UsageSnippet language="typescript" operationID="addSignerList" method="post" path="/v1/projects/{idOrName}/signer-lists" example="projectName" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.signing.addSignerList("my-project");

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { signingAddSignerList } from "@alienplatform/platform-api/funcs/signingAddSignerList.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await signingAddSignerList(alien, "my-project");
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("signingAddSignerList failed:", res.error);
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
  useSigningAddSignerListMutation
} from "@alienplatform/platform-api/react-query/signingAddSignerList.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `idOrName`                                                                                                                                                                     | *string*                                                                                                                                                                       | :heavy_check_mark:                                                                                                                                                             | Project ID or name.                                                                                                                                                            |
| `signedStatementRequest`                                                                                                                                                       | [models.SignedStatementRequest](../../models/signedstatementrequest.md)                                                                                                        | :heavy_minus_sign:                                                                                                                                                             | N/A                                                                                                                                                                            |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.AddSignerListResponse](../../models/addsignerlistresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 400, 403, 404, 409       | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## getProjectSigning

The project's signing as the dashboard and `alien devices list` show it: fingerprint, trusted devices, recovery and waiting changes. Decoded from the stored lists for display; nothing here is verified.

### Example Usage: projectId

<!-- UsageSnippet language="typescript" operationID="getProjectSigning" method="get" path="/v1/projects/{idOrName}/signing" example="projectId" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.signing.getProjectSigning({
    idOrName: "prj_mcytp6z3j91f7tn5ryqsfwtrdqkv",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { signingGetProjectSigning } from "@alienplatform/platform-api/funcs/signingGetProjectSigning.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await signingGetProjectSigning(alien, {
    idOrName: "prj_mcytp6z3j91f7tn5ryqsfwtrdqkv",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("signingGetProjectSigning failed:", res.error);
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
  useSigningGetProjectSigning,
  useSigningGetProjectSigningSuspense,

  // Utility for prefetching data during server-side rendering and in React
  // Server Components that will be immediately available to client components
  // using the hooks.
  prefetchSigningGetProjectSigning,

  // Utilities to invalidate the query cache for this query in response to
  // mutations and other user actions.
  invalidateSigningGetProjectSigning,
  invalidateAllSigningGetProjectSigning,
} from "@alienplatform/platform-api/react-query/signingGetProjectSigning.js";
```
### Example Usage: projectName

<!-- UsageSnippet language="typescript" operationID="getProjectSigning" method="get" path="/v1/projects/{idOrName}/signing" example="projectName" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.signing.getProjectSigning({
    idOrName: "my-project",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { signingGetProjectSigning } from "@alienplatform/platform-api/funcs/signingGetProjectSigning.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await signingGetProjectSigning(alien, {
    idOrName: "my-project",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("signingGetProjectSigning failed:", res.error);
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
  useSigningGetProjectSigning,
  useSigningGetProjectSigningSuspense,

  // Utility for prefetching data during server-side rendering and in React
  // Server Components that will be immediately available to client components
  // using the hooks.
  prefetchSigningGetProjectSigning,

  // Utilities to invalidate the query cache for this query in response to
  // mutations and other user actions.
  invalidateSigningGetProjectSigning,
  invalidateAllSigningGetProjectSigning,
} from "@alienplatform/platform-api/react-query/signingGetProjectSigning.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.GetProjectSigningRequest](../../models/operations/getprojectsigningrequest.md)                                                                                     | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.ProjectSigning](../../models/projectsigning.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 400, 403, 404            | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## listPendingChanges

The project's prepared signer list changes with the given status, newest first.

### Example Usage: projectId

<!-- UsageSnippet language="typescript" operationID="listPendingChanges" method="get" path="/v1/projects/{idOrName}/pending-changes" example="projectId" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.signing.listPendingChanges({
    idOrName: "prj_mcytp6z3j91f7tn5ryqsfwtrdqkv",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { signingListPendingChanges } from "@alienplatform/platform-api/funcs/signingListPendingChanges.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await signingListPendingChanges(alien, {
    idOrName: "prj_mcytp6z3j91f7tn5ryqsfwtrdqkv",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("signingListPendingChanges failed:", res.error);
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
  useSigningListPendingChanges,
  useSigningListPendingChangesSuspense,

  // Utility for prefetching data during server-side rendering and in React
  // Server Components that will be immediately available to client components
  // using the hooks.
  prefetchSigningListPendingChanges,

  // Utilities to invalidate the query cache for this query in response to
  // mutations and other user actions.
  invalidateSigningListPendingChanges,
  invalidateAllSigningListPendingChanges,
} from "@alienplatform/platform-api/react-query/signingListPendingChanges.js";
```
### Example Usage: projectName

<!-- UsageSnippet language="typescript" operationID="listPendingChanges" method="get" path="/v1/projects/{idOrName}/pending-changes" example="projectName" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.signing.listPendingChanges({
    idOrName: "my-project",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { signingListPendingChanges } from "@alienplatform/platform-api/funcs/signingListPendingChanges.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await signingListPendingChanges(alien, {
    idOrName: "my-project",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("signingListPendingChanges failed:", res.error);
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
  useSigningListPendingChanges,
  useSigningListPendingChangesSuspense,

  // Utility for prefetching data during server-side rendering and in React
  // Server Components that will be immediately available to client components
  // using the hooks.
  prefetchSigningListPendingChanges,

  // Utilities to invalidate the query cache for this query in response to
  // mutations and other user actions.
  invalidateSigningListPendingChanges,
  invalidateAllSigningListPendingChanges,
} from "@alienplatform/platform-api/react-query/signingListPendingChanges.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.ListPendingChangesRequest](../../models/operations/listpendingchangesrequest.md)                                                                                   | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.PendingChangesResponse](../../models/pendingchangesresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 400, 403, 404            | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## createPendingChange

File the body of the next signer list for an admin to sign with `alien approve`: a device asking to be trusted, a CI key, a removal or a refresh. The body is stored as received; the CLI shows the admin a diff against the list it verified.

### Example Usage: projectId

<!-- UsageSnippet language="typescript" operationID="createPendingChange" method="post" path="/v1/projects/{idOrName}/pending-changes" example="projectId" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.signing.createPendingChange("prj_mcytp6z3j91f7tn5ryqsfwtrdqkv");

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { signingCreatePendingChange } from "@alienplatform/platform-api/funcs/signingCreatePendingChange.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await signingCreatePendingChange(alien, "prj_mcytp6z3j91f7tn5ryqsfwtrdqkv");
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("signingCreatePendingChange failed:", res.error);
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
  useSigningCreatePendingChangeMutation
} from "@alienplatform/platform-api/react-query/signingCreatePendingChange.js";
```
### Example Usage: projectName

<!-- UsageSnippet language="typescript" operationID="createPendingChange" method="post" path="/v1/projects/{idOrName}/pending-changes" example="projectName" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.signing.createPendingChange("my-project");

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { signingCreatePendingChange } from "@alienplatform/platform-api/funcs/signingCreatePendingChange.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await signingCreatePendingChange(alien, "my-project");
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("signingCreatePendingChange failed:", res.error);
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
  useSigningCreatePendingChangeMutation
} from "@alienplatform/platform-api/react-query/signingCreatePendingChange.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `idOrName`                                                                                                                                                                     | *string*                                                                                                                                                                       | :heavy_check_mark:                                                                                                                                                             | Project ID or name.                                                                                                                                                            |
| `createPendingChangeRequest`                                                                                                                                                   | [models.CreatePendingChangeRequest](../../models/creatependingchangerequest.md)                                                                                                | :heavy_minus_sign:                                                                                                                                                             | N/A                                                                                                                                                                            |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.PendingChange](../../models/pendingchange.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 400, 403, 404, 409       | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## getPendingChange

### Example Usage

<!-- UsageSnippet language="typescript" operationID="getPendingChange" method="get" path="/v1/pending-changes/{id}" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.signing.getPendingChange({
    id: "pch_gz7jg0o8yf5m0b0h8uanp2tajfon",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { signingGetPendingChange } from "@alienplatform/platform-api/funcs/signingGetPendingChange.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await signingGetPendingChange(alien, {
    id: "pch_gz7jg0o8yf5m0b0h8uanp2tajfon",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("signingGetPendingChange failed:", res.error);
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
  useSigningGetPendingChange,
  useSigningGetPendingChangeSuspense,

  // Utility for prefetching data during server-side rendering and in React
  // Server Components that will be immediately available to client components
  // using the hooks.
  prefetchSigningGetPendingChange,

  // Utilities to invalidate the query cache for this query in response to
  // mutations and other user actions.
  invalidateSigningGetPendingChange,
  invalidateAllSigningGetPendingChange,
} from "@alienplatform/platform-api/react-query/signingGetPendingChange.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.GetPendingChangeRequest](../../models/operations/getpendingchangerequest.md)                                                                                       | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.PendingChange](../../models/pendingchange.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 400, 403, 404            | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## resolvePendingChange

Resolve a pending change: approved, naming the stored signer list version that carries it, or rejected. The platform checks only that the version is stored.

### Example Usage

<!-- UsageSnippet language="typescript" operationID="resolvePendingChange" method="post" path="/v1/pending-changes/{id}/resolution" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.signing.resolvePendingChange("pch_gz7jg0o8yf5m0b0h8uanp2tajfon");

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { signingResolvePendingChange } from "@alienplatform/platform-api/funcs/signingResolvePendingChange.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await signingResolvePendingChange(alien, "pch_gz7jg0o8yf5m0b0h8uanp2tajfon");
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("signingResolvePendingChange failed:", res.error);
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
  useSigningResolvePendingChangeMutation
} from "@alienplatform/platform-api/react-query/signingResolvePendingChange.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    | Example                                                                                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `id`                                                                                                                                                                           | *string*                                                                                                                                                                       | :heavy_check_mark:                                                                                                                                                             | Unique identifier for the pending change.                                                                                                                                      | pch_gz7jg0o8yf5m0b0h8uanp2tajfon                                                                                                                                               |
| `resolvePendingChangeRequest`                                                                                                                                                  | *models.ResolvePendingChangeRequest*                                                                                                                                           | :heavy_minus_sign:                                                                                                                                                             | N/A                                                                                                                                                                            |                                                                                                                                                                                |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |                                                                                                                                                                                |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |                                                                                                                                                                                |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |                                                                                                                                                                                |

### Response

**Promise\<[models.PendingChange](../../models/pendingchange.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 400, 403, 404, 409       | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## addDeploymentConfigStatement

Store a signed `config/v1` statement for the deployment: developer environment variables and stack inputs, signed by a trusted device or CI. The newest sequence is delivered on sync.

### Example Usage

<!-- UsageSnippet language="typescript" operationID="addDeploymentConfigStatement" method="post" path="/v1/deployments/{id}/config-statements" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  await alien.signing.addDeploymentConfigStatement("dep_0c29fq4a2yjb7kx3smwdgxlcxd7f");


}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { signingAddDeploymentConfigStatement } from "@alienplatform/platform-api/funcs/signingAddDeploymentConfigStatement.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await signingAddDeploymentConfigStatement(alien, "dep_0c29fq4a2yjb7kx3smwdgxlcxd7f");
  if (res.ok) {
    const { value: result } = res;

  } else {
    console.log("signingAddDeploymentConfigStatement failed:", res.error);
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
  useSigningAddDeploymentConfigStatementMutation
} from "@alienplatform/platform-api/react-query/signingAddDeploymentConfigStatement.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    | Example                                                                                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `id`                                                                                                                                                                           | *string*                                                                                                                                                                       | :heavy_check_mark:                                                                                                                                                             | Unique identifier for the deployment.                                                                                                                                          | dep_0c29fq4a2yjb7kx3smwdgxlcxd7f                                                                                                                                               |
| `signedStatementRequest`                                                                                                                                                       | [models.SignedStatementRequest](../../models/signedstatementrequest.md)                                                                                                        | :heavy_minus_sign:                                                                                                                                                             | N/A                                                                                                                                                                            |                                                                                                                                                                                |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |                                                                                                                                                                                |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |                                                                                                                                                                                |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |                                                                                                                                                                                |

### Response

**Promise\<void\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 400, 403, 404, 409       | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## listDeploymentConfigStatements

Every signed `config/v1` statement stored for the deployment, lowest sequence first, exactly as stored. The CLI builds the next statement on the newest and checks the chain when it rotates the CI key.

### Example Usage

<!-- UsageSnippet language="typescript" operationID="listDeploymentConfigStatements" method="get" path="/v1/deployments/{id}/config-statements" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.signing.listDeploymentConfigStatements({
    id: "dep_0c29fq4a2yjb7kx3smwdgxlcxd7f",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { signingListDeploymentConfigStatements } from "@alienplatform/platform-api/funcs/signingListDeploymentConfigStatements.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await signingListDeploymentConfigStatements(alien, {
    id: "dep_0c29fq4a2yjb7kx3smwdgxlcxd7f",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("signingListDeploymentConfigStatements failed:", res.error);
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
  useSigningListDeploymentConfigStatements,
  useSigningListDeploymentConfigStatementsSuspense,

  // Utility for prefetching data during server-side rendering and in React
  // Server Components that will be immediately available to client components
  // using the hooks.
  prefetchSigningListDeploymentConfigStatements,

  // Utilities to invalidate the query cache for this query in response to
  // mutations and other user actions.
  invalidateSigningListDeploymentConfigStatements,
  invalidateAllSigningListDeploymentConfigStatements,
} from "@alienplatform/platform-api/react-query/signingListDeploymentConfigStatements.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.ListDeploymentConfigStatementsRequest](../../models/operations/listdeploymentconfigstatementsrequest.md)                                                           | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.ConfigStatementsResponse](../../models/configstatementsresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 400, 403, 404            | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## getLatestDeploymentConfigStatement

The deployment's newest signed `config/v1` statement, exactly as stored. The manager delivers it to enforcers on sync.

### Example Usage

<!-- UsageSnippet language="typescript" operationID="getLatestDeploymentConfigStatement" method="get" path="/v1/deployments/{id}/config-statements/latest" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.signing.getLatestDeploymentConfigStatement({
    id: "dep_0c29fq4a2yjb7kx3smwdgxlcxd7f",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { signingGetLatestDeploymentConfigStatement } from "@alienplatform/platform-api/funcs/signingGetLatestDeploymentConfigStatement.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await signingGetLatestDeploymentConfigStatement(alien, {
    id: "dep_0c29fq4a2yjb7kx3smwdgxlcxd7f",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("signingGetLatestDeploymentConfigStatement failed:", res.error);
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
  useSigningGetLatestDeploymentConfigStatement,
  useSigningGetLatestDeploymentConfigStatementSuspense,

  // Utility for prefetching data during server-side rendering and in React
  // Server Components that will be immediately available to client components
  // using the hooks.
  prefetchSigningGetLatestDeploymentConfigStatement,

  // Utilities to invalidate the query cache for this query in response to
  // mutations and other user actions.
  invalidateSigningGetLatestDeploymentConfigStatement,
  invalidateAllSigningGetLatestDeploymentConfigStatement,
} from "@alienplatform/platform-api/react-query/signingGetLatestDeploymentConfigStatement.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.GetLatestDeploymentConfigStatementRequest](../../models/operations/getlatestdeploymentconfigstatementrequest.md)                                                   | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.ConfigStatementResponse](../../models/configstatementresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 400, 403, 404            | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |