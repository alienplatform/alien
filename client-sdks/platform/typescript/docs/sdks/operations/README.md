# Operations

## Overview

### Available Operations

* [listPlugins](#listplugins) - List available operations plugins (builtin + custom) for a project, with their operations and risk tiers.
* [publishPlugin](#publishplugin) - Register a version of a custom operations plugin whose bundle ZIP has already been uploaded to S3 (see POST /plugins/upload-url). Publishing changes no deployment: a release runs the version it declares in `operations()`.
* [createBundleUploadUrl](#createbundleuploadurl) - Get a presigned S3 URL to upload a custom operations plugin bundle ZIP. Upload the ZIP with a PUT to the returned url (sending the given Content-Type), then call POST /plugins to register it.
* [invoke](#invoke) - Invoke a plugin operation against a deployment. Honors the approval rules the deployment's release declares in `operations()`.
* [verifyCheck](#verifycheck) - One verification poll cycle for an original operation command. Loads that command's authoritative stored result and dispatch-time verification contract, dispatches the frozen read-only poll operation once, and evaluates its frozen success condition. Callers poll this repeatedly per the returned policy.
* [createAccessRequest](#createaccessrequest) - Create an access request — either plan-backed (an ai-agent investigation's exact commands) or plan-less (a CLI-originated exact operation or wildcard pattern, resolved and frozen here). Plan-backed requests await the engineer gate (status `pending-approval`); plan-less requests are queued immediately since the requester is asking for their own access (status `queued`).
* [listAccessRequests](#listaccessrequests) - List a project's access requests, newest first.
* [queueAccessRequest](#queueaccessrequest) - Engineer gate — approve a pending access request, queuing it for the operator to materialize. Records who queued it.
* [approveAccessRequest](#approveaccessrequest) - Customer gate — an authenticated workspace member or administrator other than the requester may approve a queued access request. Actor identity comes from authentication; method/source are audit context only.
* [denyAccessRequest](#denyaccessrequest) - Customer gate — an authenticated workspace member or administrator other than the requester may reject a queued access request. Actor identity comes from authentication.
* [getAccessRequestCoordinates](#getaccessrequestcoordinates) - The customer's kubectl approve command for a queued access request, or null until the operator has materialized the grant CR and reported its coordinates. Polled by the Slack handler to update the access-plan card.
* [getLiveDebugGrant](#getlivedebuggrant) - Find an approved, unexpired access request the caller created whose debug grant matches this deployment and tool (and, when given, namespace/cloudScope). Returns the most recently approved match, or 404 when none is live. Used to reuse an existing grant instead of proposing a new access request. Only the requester's own grants match: a debug session must present a grant its caller requested, so another principal's grant is never returned here.
* [getAccessRequestActivity](#getaccessrequestactivity) - The operation commands and debug sessions an access request authorized, with each command's verification outcome. Authorized like getAccessRequest.
* [revokeAccessRequest](#revokeaccessrequest) - Revoke a pending, queued or customer-approved access request. The principal that created the request, or any workspace member or administrator who can read its project, may revoke it. Commands the grant authorized that have not been dispatched are failed; dispatched commands finish. Linked debug sessions are stopped and refuse their next request. Revoking an already revoked request returns it unchanged.
* [getAccessRequest](#getaccessrequest) - Get an access request by id.
* [~~setBuiltinPlugins~~](#setbuiltinplugins) - Replace the complete set of enabled built-in operations plugins for a project. Returns the cloud permission delta versus the previously enabled set. :warning: **Deprecated**
* [~~setPluginEnabled~~](#setpluginenabled) - Enable or disable an operations plugin (builtin or custom) for a project. Only enabled plugins are distributed to Operators and can be invoked. Returns the cloud permission delta versus the previously enabled set. With `dryRun`, validates the change and returns the delta without saving it. :warning: **Deprecated**
* [~~getPolicy~~](#getpolicy) - Get a project's per-command approval policy. Mirrors what the operator enforces: `plugin/operation` / `plugin/*` / `*` patterns → auto | manual. :warning: **Deprecated**
* [~~updatePolicy~~](#updatepolicy) - Replace a project's per-command approval policy (full rule set). Patterns are `plugin/operation`, `plugin/*`, or `*`; each maps to auto | manual. :warning: **Deprecated**

## listPlugins

List available operations plugins (builtin + custom) for a project, with their operations and risk tiers.

### Example Usage: projectId

<!-- UsageSnippet language="typescript" operationID="listOperationsPlugins" method="get" path="/v1/operations/plugins" example="projectId" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.operations.listPlugins({
    project: "prj_mcytp6z3j91f7tn5ryqsfwtr",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { operationsListPlugins } from "@alienplatform/platform-api/funcs/operationsListPlugins.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await operationsListPlugins(alien, {
    project: "prj_mcytp6z3j91f7tn5ryqsfwtr",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("operationsListPlugins failed:", res.error);
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
  useOperationsListPlugins,
  useOperationsListPluginsSuspense,

  // Utility for prefetching data during server-side rendering and in React
  // Server Components that will be immediately available to client components
  // using the hooks.
  prefetchOperationsListPlugins,
  
  // Utilities to invalidate the query cache for this query in response to
  // mutations and other user actions.
  invalidateOperationsListPlugins,
  invalidateAllOperationsListPlugins,
} from "@alienplatform/platform-api/react-query/operationsListPlugins.js";
```
### Example Usage: projectName

<!-- UsageSnippet language="typescript" operationID="listOperationsPlugins" method="get" path="/v1/operations/plugins" example="projectName" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.operations.listPlugins({
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
import { operationsListPlugins } from "@alienplatform/platform-api/funcs/operationsListPlugins.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await operationsListPlugins(alien, {
    project: "my-project",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("operationsListPlugins failed:", res.error);
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
  useOperationsListPlugins,
  useOperationsListPluginsSuspense,

  // Utility for prefetching data during server-side rendering and in React
  // Server Components that will be immediately available to client components
  // using the hooks.
  prefetchOperationsListPlugins,
  
  // Utilities to invalidate the query cache for this query in response to
  // mutations and other user actions.
  invalidateOperationsListPlugins,
  invalidateAllOperationsListPlugins,
} from "@alienplatform/platform-api/react-query/operationsListPlugins.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.ListOperationsPluginsRequest](../../models/operations/listoperationspluginsrequest.md)                                                                             | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.ListOperationsPluginsResponse](../../models/listoperationspluginsresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 409                      | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## publishPlugin

Register a version of a custom operations plugin whose bundle ZIP has already been uploaded to S3 (see POST /plugins/upload-url). Publishing changes no deployment: a release runs the version it declares in `operations()`.

### Example Usage: projectId

<!-- UsageSnippet language="typescript" operationID="publishOperationsPlugin" method="post" path="/v1/operations/plugins" example="projectId" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.operations.publishPlugin("prj_mcytp6z3j91f7tn5ryqsfwtr");

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { operationsPublishPlugin } from "@alienplatform/platform-api/funcs/operationsPublishPlugin.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await operationsPublishPlugin(alien, "prj_mcytp6z3j91f7tn5ryqsfwtr");
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("operationsPublishPlugin failed:", res.error);
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
  useOperationsPublishPluginMutation
} from "@alienplatform/platform-api/react-query/operationsPublishPlugin.js";
```
### Example Usage: projectName

<!-- UsageSnippet language="typescript" operationID="publishOperationsPlugin" method="post" path="/v1/operations/plugins" example="projectName" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.operations.publishPlugin("my-project");

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { operationsPublishPlugin } from "@alienplatform/platform-api/funcs/operationsPublishPlugin.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await operationsPublishPlugin(alien, "my-project");
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("operationsPublishPlugin failed:", res.error);
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
  useOperationsPublishPluginMutation
} from "@alienplatform/platform-api/react-query/operationsPublishPlugin.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `project`                                                                                                                                                                      | *string*                                                                                                                                                                       | :heavy_check_mark:                                                                                                                                                             | Filter by project ID or name.                                                                                                                                                  |
| `publishOperationsPluginRequest`                                                                                                                                               | [models.PublishOperationsPluginRequest](../../models/publishoperationspluginrequest.md)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | N/A                                                                                                                                                                            |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.PublishOperationsPluginResponse](../../models/publishoperationspluginresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 400, 402, 409            | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## createBundleUploadUrl

Get a presigned S3 URL to upload a custom operations plugin bundle ZIP. Upload the ZIP with a PUT to the returned url (sending the given Content-Type), then call POST /plugins to register it.

### Example Usage: projectId

<!-- UsageSnippet language="typescript" operationID="createOperationsBundleUploadUrl" method="post" path="/v1/operations/plugins/upload-url" example="projectId" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.operations.createBundleUploadUrl("prj_mcytp6z3j91f7tn5ryqsfwtr");

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { operationsCreateBundleUploadUrl } from "@alienplatform/platform-api/funcs/operationsCreateBundleUploadUrl.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await operationsCreateBundleUploadUrl(alien, "prj_mcytp6z3j91f7tn5ryqsfwtr");
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("operationsCreateBundleUploadUrl failed:", res.error);
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
  useOperationsCreateBundleUploadUrlMutation
} from "@alienplatform/platform-api/react-query/operationsCreateBundleUploadUrl.js";
```
### Example Usage: projectName

<!-- UsageSnippet language="typescript" operationID="createOperationsBundleUploadUrl" method="post" path="/v1/operations/plugins/upload-url" example="projectName" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.operations.createBundleUploadUrl("my-project");

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { operationsCreateBundleUploadUrl } from "@alienplatform/platform-api/funcs/operationsCreateBundleUploadUrl.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await operationsCreateBundleUploadUrl(alien, "my-project");
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("operationsCreateBundleUploadUrl failed:", res.error);
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
  useOperationsCreateBundleUploadUrlMutation
} from "@alienplatform/platform-api/react-query/operationsCreateBundleUploadUrl.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `project`                                                                                                                                                                      | *string*                                                                                                                                                                       | :heavy_check_mark:                                                                                                                                                             | Filter by project ID or name.                                                                                                                                                  |
| `operationsBundleUploadUrlRequest`                                                                                                                                             | [models.OperationsBundleUploadUrlRequest](../../models/operationsbundleuploadurlrequest.md)                                                                                    | :heavy_minus_sign:                                                                                                                                                             | N/A                                                                                                                                                                            |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.OperationsBundleUploadUrlResponse](../../models/operationsbundleuploadurlresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 402                      | application/json         |
| errors.APIError          | 503                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## invoke

Invoke a plugin operation against a deployment. Honors the approval rules the deployment's release declares in `operations()`.

### Example Usage: projectId

<!-- UsageSnippet language="typescript" operationID="invokeOperation" method="post" path="/v1/operations/invoke" example="projectId" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.operations.invoke("prj_mcytp6z3j91f7tn5ryqsfwtr");

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { operationsInvoke } from "@alienplatform/platform-api/funcs/operationsInvoke.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await operationsInvoke(alien, "prj_mcytp6z3j91f7tn5ryqsfwtr");
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("operationsInvoke failed:", res.error);
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
  useOperationsInvokeMutation
} from "@alienplatform/platform-api/react-query/operationsInvoke.js";
```
### Example Usage: projectName

<!-- UsageSnippet language="typescript" operationID="invokeOperation" method="post" path="/v1/operations/invoke" example="projectName" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.operations.invoke("my-project");

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { operationsInvoke } from "@alienplatform/platform-api/funcs/operationsInvoke.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await operationsInvoke(alien, "my-project");
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("operationsInvoke failed:", res.error);
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
  useOperationsInvokeMutation
} from "@alienplatform/platform-api/react-query/operationsInvoke.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `project`                                                                                                                                                                      | *string*                                                                                                                                                                       | :heavy_check_mark:                                                                                                                                                             | Filter by project ID or name.                                                                                                                                                  |
| `invokeOperationRequest`                                                                                                                                                       | [models.InvokeOperationRequest](../../models/invokeoperationrequest.md)                                                                                                        | :heavy_minus_sign:                                                                                                                                                             | N/A                                                                                                                                                                            |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.InvokeOperationResponse](../../models/invokeoperationresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 400, 402, 403, 404, 409  | application/json         |
| errors.APIError          | 500, 502                 | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## verifyCheck

One verification poll cycle for an original operation command. Loads that command's authoritative stored result and dispatch-time verification contract, dispatches the frozen read-only poll operation once, and evaluates its frozen success condition. Callers poll this repeatedly per the returned policy.

### Example Usage: projectId

<!-- UsageSnippet language="typescript" operationID="verifyOperationCheck" method="post" path="/v1/operations/verify-check" example="projectId" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.operations.verifyCheck("prj_mcytp6z3j91f7tn5ryqsfwtr", {
    deploymentId: "<id>",
    commandId: "cmd_2sxjXxvOYct7IohT3ukliAzf",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { operationsVerifyCheck } from "@alienplatform/platform-api/funcs/operationsVerifyCheck.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await operationsVerifyCheck(alien, "prj_mcytp6z3j91f7tn5ryqsfwtr", {
    deploymentId: "<id>",
    commandId: "cmd_2sxjXxvOYct7IohT3ukliAzf",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("operationsVerifyCheck failed:", res.error);
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
  useOperationsVerifyCheckMutation
} from "@alienplatform/platform-api/react-query/operationsVerifyCheck.js";
```
### Example Usage: projectName

<!-- UsageSnippet language="typescript" operationID="verifyOperationCheck" method="post" path="/v1/operations/verify-check" example="projectName" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.operations.verifyCheck("my-project", {
    deploymentId: "<id>",
    commandId: "cmd_2sxjXxvOYct7IohT3ukliAzf",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { operationsVerifyCheck } from "@alienplatform/platform-api/funcs/operationsVerifyCheck.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await operationsVerifyCheck(alien, "my-project", {
    deploymentId: "<id>",
    commandId: "cmd_2sxjXxvOYct7IohT3ukliAzf",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("operationsVerifyCheck failed:", res.error);
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
  useOperationsVerifyCheckMutation
} from "@alienplatform/platform-api/react-query/operationsVerifyCheck.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `project`                                                                                                                                                                      | *string*                                                                                                                                                                       | :heavy_check_mark:                                                                                                                                                             | Filter by project ID or name.                                                                                                                                                  |
| `verifyOperationCheckRequest`                                                                                                                                                  | [models.VerifyOperationCheckRequest](../../models/verifyoperationcheckrequest.md)                                                                                              | :heavy_minus_sign:                                                                                                                                                             | N/A                                                                                                                                                                            |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.VerifyOperationCheckResponse](../../models/verifyoperationcheckresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 400, 403, 404            | application/json         |
| errors.APIError          | 500, 502                 | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## createAccessRequest

Create an access request — either plan-backed (an ai-agent investigation's exact commands) or plan-less (a CLI-originated exact operation or wildcard pattern, resolved and frozen here). Plan-backed requests await the engineer gate (status `pending-approval`); plan-less requests are queued immediately since the requester is asking for their own access (status `queued`).

### Example Usage

<!-- UsageSnippet language="typescript" operationID="createAccessRequest" method="post" path="/v1/access-requests" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.operations.createAccessRequest({
    deploymentId: "<id>",
    commands: [],
    operation: "kubernetes/restart-pod",
    operationPattern: "kubernetes/*",
    debugNamespace: "braintrust",
    debugCloudScope: "123456789012/prod-readonly",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { operationsCreateAccessRequest } from "@alienplatform/platform-api/funcs/operationsCreateAccessRequest.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await operationsCreateAccessRequest(alien, {
    deploymentId: "<id>",
    commands: [],
    operation: "kubernetes/restart-pod",
    operationPattern: "kubernetes/*",
    debugNamespace: "braintrust",
    debugCloudScope: "123456789012/prod-readonly",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("operationsCreateAccessRequest failed:", res.error);
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
  useOperationsCreateAccessRequestMutation
} from "@alienplatform/platform-api/react-query/operationsCreateAccessRequest.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [models.CreateAccessRequest](../../models/createaccessrequest.md)                                                                                                              | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[operations.CreateAccessRequestResponse](../../models/operations/createaccessrequestresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 400, 403, 404, 409       | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## listAccessRequests

List a project's access requests, newest first.

### Example Usage: projectId

<!-- UsageSnippet language="typescript" operationID="listAccessRequests" method="get" path="/v1/access-requests" example="projectId" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.operations.listAccessRequests({
    project: "prj_mcytp6z3j91f7tn5ryqsfwtr",
    deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { operationsListAccessRequests } from "@alienplatform/platform-api/funcs/operationsListAccessRequests.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await operationsListAccessRequests(alien, {
    project: "prj_mcytp6z3j91f7tn5ryqsfwtr",
    deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("operationsListAccessRequests failed:", res.error);
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
  useOperationsListAccessRequests,
  useOperationsListAccessRequestsSuspense,

  // Utility for prefetching data during server-side rendering and in React
  // Server Components that will be immediately available to client components
  // using the hooks.
  prefetchOperationsListAccessRequests,
  
  // Utilities to invalidate the query cache for this query in response to
  // mutations and other user actions.
  invalidateOperationsListAccessRequests,
  invalidateAllOperationsListAccessRequests,
} from "@alienplatform/platform-api/react-query/operationsListAccessRequests.js";
```
### Example Usage: projectName

<!-- UsageSnippet language="typescript" operationID="listAccessRequests" method="get" path="/v1/access-requests" example="projectName" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.operations.listAccessRequests({
    project: "my-project",
    deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { operationsListAccessRequests } from "@alienplatform/platform-api/funcs/operationsListAccessRequests.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await operationsListAccessRequests(alien, {
    project: "my-project",
    deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("operationsListAccessRequests failed:", res.error);
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
  useOperationsListAccessRequests,
  useOperationsListAccessRequestsSuspense,

  // Utility for prefetching data during server-side rendering and in React
  // Server Components that will be immediately available to client components
  // using the hooks.
  prefetchOperationsListAccessRequests,
  
  // Utilities to invalidate the query cache for this query in response to
  // mutations and other user actions.
  invalidateOperationsListAccessRequests,
  invalidateAllOperationsListAccessRequests,
} from "@alienplatform/platform-api/react-query/operationsListAccessRequests.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.ListAccessRequestsRequest](../../models/operations/listaccessrequestsrequest.md)                                                                                   | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[operations.ListAccessRequestsResponse](../../models/operations/listaccessrequestsresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 400, 404                 | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## queueAccessRequest

Engineer gate — approve a pending access request, queuing it for the operator to materialize. Records who queued it.

### Example Usage

<!-- UsageSnippet language="typescript" operationID="queueAccessRequest" method="post" path="/v1/access-requests/{id}/queue" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.operations.queueAccessRequest({
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
import { operationsQueueAccessRequest } from "@alienplatform/platform-api/funcs/operationsQueueAccessRequest.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await operationsQueueAccessRequest(alien, {
    id: "<id>",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("operationsQueueAccessRequest failed:", res.error);
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
  useOperationsQueueAccessRequestMutation
} from "@alienplatform/platform-api/react-query/operationsQueueAccessRequest.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.QueueAccessRequestRequest](../../models/operations/queueaccessrequestrequest.md)                                                                                   | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[operations.QueueAccessRequestResponse](../../models/operations/queueaccessrequestresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 400, 403, 404, 409       | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## approveAccessRequest

Customer gate — an authenticated workspace member or administrator other than the requester may approve a queued access request. Actor identity comes from authentication; method/source are audit context only.

### Example Usage

<!-- UsageSnippet language="typescript" operationID="approveAccessRequest" method="post" path="/v1/access-requests/{id}/approve" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.operations.approveAccessRequest("<id>", {
    method: "slack",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { operationsApproveAccessRequest } from "@alienplatform/platform-api/funcs/operationsApproveAccessRequest.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await operationsApproveAccessRequest(alien, "<id>", {
    method: "slack",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("operationsApproveAccessRequest failed:", res.error);
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
  useOperationsApproveAccessRequestMutation
} from "@alienplatform/platform-api/react-query/operationsApproveAccessRequest.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `id`                                                                                                                                                                           | *string*                                                                                                                                                                       | :heavy_check_mark:                                                                                                                                                             | N/A                                                                                                                                                                            |
| `requestBody`                                                                                                                                                                  | [operations.ApproveAccessRequestRequestBody](../../models/operations/approveaccessrequestrequestbody.md)                                                                       | :heavy_minus_sign:                                                                                                                                                             | N/A                                                                                                                                                                            |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[operations.ApproveAccessRequestResponse](../../models/operations/approveaccessrequestresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 400, 403, 404, 409       | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## denyAccessRequest

Customer gate — an authenticated workspace member or administrator other than the requester may reject a queued access request. Actor identity comes from authentication.

### Example Usage

<!-- UsageSnippet language="typescript" operationID="denyAccessRequest" method="post" path="/v1/access-requests/{id}/deny" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.operations.denyAccessRequest("<id>", {
    method: "slack",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { operationsDenyAccessRequest } from "@alienplatform/platform-api/funcs/operationsDenyAccessRequest.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await operationsDenyAccessRequest(alien, "<id>", {
    method: "slack",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("operationsDenyAccessRequest failed:", res.error);
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
  useOperationsDenyAccessRequestMutation
} from "@alienplatform/platform-api/react-query/operationsDenyAccessRequest.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `id`                                                                                                                                                                           | *string*                                                                                                                                                                       | :heavy_check_mark:                                                                                                                                                             | N/A                                                                                                                                                                            |
| `requestBody`                                                                                                                                                                  | [operations.DenyAccessRequestRequestBody](../../models/operations/denyaccessrequestrequestbody.md)                                                                             | :heavy_minus_sign:                                                                                                                                                             | N/A                                                                                                                                                                            |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[operations.DenyAccessRequestResponse](../../models/operations/denyaccessrequestresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 403, 404, 409            | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## getAccessRequestCoordinates

The customer's kubectl approve command for a queued access request, or null until the operator has materialized the grant CR and reported its coordinates. Polled by the Slack handler to update the access-plan card.

### Example Usage

<!-- UsageSnippet language="typescript" operationID="getAccessRequestCoordinates" method="get" path="/v1/access-requests/{id}/coordinates" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.operations.getAccessRequestCoordinates({
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
import { operationsGetAccessRequestCoordinates } from "@alienplatform/platform-api/funcs/operationsGetAccessRequestCoordinates.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await operationsGetAccessRequestCoordinates(alien, {
    id: "<id>",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("operationsGetAccessRequestCoordinates failed:", res.error);
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
  useOperationsGetAccessRequestCoordinates,
  useOperationsGetAccessRequestCoordinatesSuspense,

  // Utility for prefetching data during server-side rendering and in React
  // Server Components that will be immediately available to client components
  // using the hooks.
  prefetchOperationsGetAccessRequestCoordinates,
  
  // Utilities to invalidate the query cache for this query in response to
  // mutations and other user actions.
  invalidateOperationsGetAccessRequestCoordinates,
  invalidateAllOperationsGetAccessRequestCoordinates,
} from "@alienplatform/platform-api/react-query/operationsGetAccessRequestCoordinates.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.GetAccessRequestCoordinatesRequest](../../models/operations/getaccessrequestcoordinatesrequest.md)                                                                 | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[operations.GetAccessRequestCoordinatesResponse](../../models/operations/getaccessrequestcoordinatesresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 404                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## getLiveDebugGrant

Find an approved, unexpired access request the caller created whose debug grant matches this deployment and tool (and, when given, namespace/cloudScope). Returns the most recently approved match, or 404 when none is live. Used to reuse an existing grant instead of proposing a new access request. Only the requester's own grants match: a debug session must present a grant its caller requested, so another principal's grant is never returned here.

### Example Usage

<!-- UsageSnippet language="typescript" operationID="getLiveDebugGrant" method="get" path="/v1/access-requests/live-debug-grant" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.operations.getLiveDebugGrant({
    deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
    debugTool: "gcloud",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { operationsGetLiveDebugGrant } from "@alienplatform/platform-api/funcs/operationsGetLiveDebugGrant.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await operationsGetLiveDebugGrant(alien, {
    deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
    debugTool: "gcloud",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("operationsGetLiveDebugGrant failed:", res.error);
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
  useOperationsGetLiveDebugGrant,
  useOperationsGetLiveDebugGrantSuspense,

  // Utility for prefetching data during server-side rendering and in React
  // Server Components that will be immediately available to client components
  // using the hooks.
  prefetchOperationsGetLiveDebugGrant,
  
  // Utilities to invalidate the query cache for this query in response to
  // mutations and other user actions.
  invalidateOperationsGetLiveDebugGrant,
  invalidateAllOperationsGetLiveDebugGrant,
} from "@alienplatform/platform-api/react-query/operationsGetLiveDebugGrant.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.GetLiveDebugGrantRequest](../../models/operations/getlivedebuggrantrequest.md)                                                                                     | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[operations.GetLiveDebugGrantResponse](../../models/operations/getlivedebuggrantresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 404                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## getAccessRequestActivity

The operation commands and debug sessions an access request authorized, with each command's verification outcome. Authorized like getAccessRequest.

### Example Usage

<!-- UsageSnippet language="typescript" operationID="getAccessRequestActivity" method="get" path="/v1/access-requests/{id}/activity" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.operations.getAccessRequestActivity({
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
import { operationsGetAccessRequestActivity } from "@alienplatform/platform-api/funcs/operationsGetAccessRequestActivity.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await operationsGetAccessRequestActivity(alien, {
    id: "<id>",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("operationsGetAccessRequestActivity failed:", res.error);
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
  useOperationsGetAccessRequestActivity,
  useOperationsGetAccessRequestActivitySuspense,

  // Utility for prefetching data during server-side rendering and in React
  // Server Components that will be immediately available to client components
  // using the hooks.
  prefetchOperationsGetAccessRequestActivity,
  
  // Utilities to invalidate the query cache for this query in response to
  // mutations and other user actions.
  invalidateOperationsGetAccessRequestActivity,
  invalidateAllOperationsGetAccessRequestActivity,
} from "@alienplatform/platform-api/react-query/operationsGetAccessRequestActivity.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.GetAccessRequestActivityRequest](../../models/operations/getaccessrequestactivityrequest.md)                                                                       | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.AccessRequestActivity](../../models/accessrequestactivity.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 404                      | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## revokeAccessRequest

Revoke a pending, queued or customer-approved access request. The principal that created the request, or any workspace member or administrator who can read its project, may revoke it. Commands the grant authorized that have not been dispatched are failed; dispatched commands finish. Linked debug sessions are stopped and refuse their next request. Revoking an already revoked request returns it unchanged.

### Example Usage

<!-- UsageSnippet language="typescript" operationID="revokeAccessRequest" method="post" path="/v1/access-requests/{id}/revoke" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.operations.revokeAccessRequest("<id>");

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { operationsRevokeAccessRequest } from "@alienplatform/platform-api/funcs/operationsRevokeAccessRequest.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await operationsRevokeAccessRequest(alien, "<id>");
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("operationsRevokeAccessRequest failed:", res.error);
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
  useOperationsRevokeAccessRequestMutation
} from "@alienplatform/platform-api/react-query/operationsRevokeAccessRequest.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `id`                                                                                                                                                                           | *string*                                                                                                                                                                       | :heavy_check_mark:                                                                                                                                                             | N/A                                                                                                                                                                            |
| `revokeAccessRequest`                                                                                                                                                          | [models.RevokeAccessRequest](../../models/revokeaccessrequest.md)                                                                                                              | :heavy_minus_sign:                                                                                                                                                             | N/A                                                                                                                                                                            |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[operations.RevokeAccessRequestResponse](../../models/operations/revokeaccessrequestresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 403, 404, 409            | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## getAccessRequest

Get an access request by id.

### Example Usage

<!-- UsageSnippet language="typescript" operationID="getAccessRequest" method="get" path="/v1/access-requests/{id}" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.operations.getAccessRequest({
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
import { operationsGetAccessRequest } from "@alienplatform/platform-api/funcs/operationsGetAccessRequest.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await operationsGetAccessRequest(alien, {
    id: "<id>",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("operationsGetAccessRequest failed:", res.error);
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
  useOperationsGetAccessRequest,
  useOperationsGetAccessRequestSuspense,

  // Utility for prefetching data during server-side rendering and in React
  // Server Components that will be immediately available to client components
  // using the hooks.
  prefetchOperationsGetAccessRequest,
  
  // Utilities to invalidate the query cache for this query in response to
  // mutations and other user actions.
  invalidateOperationsGetAccessRequest,
  invalidateAllOperationsGetAccessRequest,
} from "@alienplatform/platform-api/react-query/operationsGetAccessRequest.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.GetAccessRequestRequest](../../models/operations/getaccessrequestrequest.md)                                                                                       | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[operations.GetAccessRequestResponse](../../models/operations/getaccessrequestresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 404                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## ~~setBuiltinPlugins~~

Replace the complete set of enabled built-in operations plugins for a project. Returns the cloud permission delta versus the previously enabled set.

> :warning: **DEPRECATED**: This will be removed in a future release, please migrate away from it as soon as possible.

### Example Usage: projectId

<!-- UsageSnippet language="typescript" operationID="setBuiltinOperationsPlugins" method="put" path="/v1/operations/plugins/builtin/enabled" example="projectId" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.operations.setBuiltinPlugins({
    project: "prj_mcytp6z3j91f7tn5ryqsfwtr",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { operationsSetBuiltinPlugins } from "@alienplatform/platform-api/funcs/operationsSetBuiltinPlugins.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await operationsSetBuiltinPlugins(alien, {
    project: "prj_mcytp6z3j91f7tn5ryqsfwtr",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("operationsSetBuiltinPlugins failed:", res.error);
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
  useOperationsSetBuiltinPluginsMutation
} from "@alienplatform/platform-api/react-query/operationsSetBuiltinPlugins.js";
```
### Example Usage: projectName

<!-- UsageSnippet language="typescript" operationID="setBuiltinOperationsPlugins" method="put" path="/v1/operations/plugins/builtin/enabled" example="projectName" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.operations.setBuiltinPlugins({
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
import { operationsSetBuiltinPlugins } from "@alienplatform/platform-api/funcs/operationsSetBuiltinPlugins.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await operationsSetBuiltinPlugins(alien, {
    project: "my-project",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("operationsSetBuiltinPlugins failed:", res.error);
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
  useOperationsSetBuiltinPluginsMutation
} from "@alienplatform/platform-api/react-query/operationsSetBuiltinPlugins.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.SetBuiltinOperationsPluginsRequest](../../models/operations/setbuiltinoperationspluginsrequest.md)                                                                 | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.SetBuiltinOperationsPluginsResponse](../../models/setbuiltinoperationspluginsresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 400                      | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## ~~setPluginEnabled~~

Enable or disable an operations plugin (builtin or custom) for a project. Only enabled plugins are distributed to Operators and can be invoked. Returns the cloud permission delta versus the previously enabled set. With `dryRun`, validates the change and returns the delta without saving it.

> :warning: **DEPRECATED**: This will be removed in a future release, please migrate away from it as soon as possible.

### Example Usage: projectId

<!-- UsageSnippet language="typescript" operationID="setOperationsPluginEnabled" method="patch" path="/v1/operations/plugins/{name}/enabled" example="projectId" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.operations.setPluginEnabled({
    name: "<value>",
    project: "prj_mcytp6z3j91f7tn5ryqsfwtr",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { operationsSetPluginEnabled } from "@alienplatform/platform-api/funcs/operationsSetPluginEnabled.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await operationsSetPluginEnabled(alien, {
    name: "<value>",
    project: "prj_mcytp6z3j91f7tn5ryqsfwtr",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("operationsSetPluginEnabled failed:", res.error);
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
  useOperationsSetPluginEnabledMutation
} from "@alienplatform/platform-api/react-query/operationsSetPluginEnabled.js";
```
### Example Usage: projectName

<!-- UsageSnippet language="typescript" operationID="setOperationsPluginEnabled" method="patch" path="/v1/operations/plugins/{name}/enabled" example="projectName" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.operations.setPluginEnabled({
    name: "<value>",
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
import { operationsSetPluginEnabled } from "@alienplatform/platform-api/funcs/operationsSetPluginEnabled.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await operationsSetPluginEnabled(alien, {
    name: "<value>",
    project: "my-project",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("operationsSetPluginEnabled failed:", res.error);
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
  useOperationsSetPluginEnabledMutation
} from "@alienplatform/platform-api/react-query/operationsSetPluginEnabled.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.SetOperationsPluginEnabledRequest](../../models/operations/setoperationspluginenabledrequest.md)                                                                   | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.SetOperationsPluginEnabledResponse](../../models/setoperationspluginenabledresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 404, 409                 | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## ~~getPolicy~~

Get a project's per-command approval policy. Mirrors what the operator enforces: `plugin/operation` / `plugin/*` / `*` patterns → auto | manual.

> :warning: **DEPRECATED**: This will be removed in a future release, please migrate away from it as soon as possible.

### Example Usage: projectId

<!-- UsageSnippet language="typescript" operationID="getOperationsPolicy" method="get" path="/v1/operations/policy" example="projectId" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.operations.getPolicy({
    project: "prj_mcytp6z3j91f7tn5ryqsfwtr",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { operationsGetPolicy } from "@alienplatform/platform-api/funcs/operationsGetPolicy.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await operationsGetPolicy(alien, {
    project: "prj_mcytp6z3j91f7tn5ryqsfwtr",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("operationsGetPolicy failed:", res.error);
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
  useOperationsGetPolicy,
  useOperationsGetPolicySuspense,

  // Utility for prefetching data during server-side rendering and in React
  // Server Components that will be immediately available to client components
  // using the hooks.
  prefetchOperationsGetPolicy,
  
  // Utilities to invalidate the query cache for this query in response to
  // mutations and other user actions.
  invalidateOperationsGetPolicy,
  invalidateAllOperationsGetPolicy,
} from "@alienplatform/platform-api/react-query/operationsGetPolicy.js";
```
### Example Usage: projectName

<!-- UsageSnippet language="typescript" operationID="getOperationsPolicy" method="get" path="/v1/operations/policy" example="projectName" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.operations.getPolicy({
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
import { operationsGetPolicy } from "@alienplatform/platform-api/funcs/operationsGetPolicy.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await operationsGetPolicy(alien, {
    project: "my-project",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("operationsGetPolicy failed:", res.error);
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
  useOperationsGetPolicy,
  useOperationsGetPolicySuspense,

  // Utility for prefetching data during server-side rendering and in React
  // Server Components that will be immediately available to client components
  // using the hooks.
  prefetchOperationsGetPolicy,
  
  // Utilities to invalidate the query cache for this query in response to
  // mutations and other user actions.
  invalidateOperationsGetPolicy,
  invalidateAllOperationsGetPolicy,
} from "@alienplatform/platform-api/react-query/operationsGetPolicy.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.GetOperationsPolicyRequest](../../models/operations/getoperationspolicyrequest.md)                                                                                 | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.OperationsPolicyResponse](../../models/operationspolicyresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## ~~updatePolicy~~

Replace a project's per-command approval policy (full rule set). Patterns are `plugin/operation`, `plugin/*`, or `*`; each maps to auto | manual.

> :warning: **DEPRECATED**: This will be removed in a future release, please migrate away from it as soon as possible.

### Example Usage: projectId

<!-- UsageSnippet language="typescript" operationID="updateOperationsPolicy" method="put" path="/v1/operations/policy" example="projectId" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.operations.updatePolicy({
    project: "prj_mcytp6z3j91f7tn5ryqsfwtr",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { operationsUpdatePolicy } from "@alienplatform/platform-api/funcs/operationsUpdatePolicy.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await operationsUpdatePolicy(alien, {
    project: "prj_mcytp6z3j91f7tn5ryqsfwtr",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("operationsUpdatePolicy failed:", res.error);
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
  useOperationsUpdatePolicyMutation
} from "@alienplatform/platform-api/react-query/operationsUpdatePolicy.js";
```
### Example Usage: projectName

<!-- UsageSnippet language="typescript" operationID="updateOperationsPolicy" method="put" path="/v1/operations/policy" example="projectName" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.operations.updatePolicy({
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
import { operationsUpdatePolicy } from "@alienplatform/platform-api/funcs/operationsUpdatePolicy.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await operationsUpdatePolicy(alien, {
    project: "my-project",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("operationsUpdatePolicy failed:", res.error);
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
  useOperationsUpdatePolicyMutation
} from "@alienplatform/platform-api/react-query/operationsUpdatePolicy.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.UpdateOperationsPolicyRequest](../../models/operations/updateoperationspolicyrequest.md)                                                                           | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.OperationsPolicyResponse](../../models/operationspolicyresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 400                      | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |