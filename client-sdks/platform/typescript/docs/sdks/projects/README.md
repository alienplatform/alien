# Projects

## Overview

### Available Operations

* [list](#list) - Retrieve all projects.
* [create](#create) - Create a new project.
* [get](#get) - Retrieve a project by ID or name.
* [update](#update) - Update a project.
* [delete](#delete) - Delete a project. The project must have no deployments.
* [getGcpOAuthProvider](#getgcpoauthprovider) - Retrieve redacted project-level Google Cloud OAuth provider settings.
* [updateGcpOAuthProvider](#updategcpoauthprovider) - Update project-level Google Cloud OAuth provider settings.
* [configureSource](#configuresource) - Connect a GitHub repository or Alien template to an existing project.
* [getDeploymentPortalDomain](#getdeploymentportaldomain) - Get the deployment portal domain binding for a project.
* [createFromTemplate](#createfromtemplate) - Create a project by forking alienplatform/alien into your namespace.
* [getTemplateUrls](#gettemplateurls) - Get template URLs for deploying setup stacks in this project.
* [getDeploymentLinkSetup](#getdeploymentlinksetup) - Get the active release stack and portal-visible setup availability for deployment-link configuration.
* [getActiveRelease](#getactiverelease) - Get the production channel's current release. When deploymentId is provided, returns that deployment's effective release: its pin, or its followed channel's current release.
* [previewModelsImpact](#previewmodelsimpact) - Preview which customer model connections a configuration change may affect.
* [setCapabilities](#setcapabilities) - Set the capabilities offered by a Project. Removing a capability prevents new setup without deleting existing customer resources.
* [configureDeployments](#configuredeployments) - Enable deployments for a Project.
* [getAiProviderHeaders](#getaiproviderheaders) - Get static headers added to AI requests for each provider.
* [configureAiProviderHeaders](#configureaiproviderheaders) - Replace the static headers added to AI requests for each provider.
* [configureModels](#configuremodels) - Configure customer-owned model providers without requiring an application Release.
* [configureKeys](#configurekeys) - Enable customer-owned application encryption without requiring an application Release.
* [configureBuckets](#configurebuckets) - Enable buckets without requiring a project Release.
* [configureRegistry](#configureregistry) - Enable customer-owned container registries without requiring an application Release.
* [configureRemoteSandbox](#configureremotesandbox) - Enable a customer-owned sandbox a hosted caller can drive through Remote Bindings. Without a custom image it publishes to every cloud on Alien's default image, which the project then follows. A custom image publishes to AWS; a public one with a linux/amd64 variant also publishes to Azure, and to GCP once Alien has built its GCP image.
* [ensureSandboxBaseImageRepository](#ensuresandboxbaseimagerepository) - Ensure the project's private image repository exists and return where to push a private sandbox base image. Name the pushed image as the remote sandbox base image afterwards.
* [getCapabilityOverview](#getcapabilityoverview) - Get safe, server-derived capability status for a Project.
* [getRemoteOperatorSummary](#getremoteoperatorsummary) - Get the authoritative Remote Operator project summary
* [acceptRemoteOperatorImage](#acceptremoteoperatorimage) - Accept a reported image for a Remote Operator installation
* [acceptRemoteOperatorPermissions](#acceptremoteoperatorpermissions) - Record that a Remote Operator installation's setup was re-applied
* [getAiUsage](#getaiusage)
* [getEncryptionUsage](#getencryptionusage)
* [getSandboxMetrics](#getsandboxmetrics)

## list

Retrieve all projects.

### Example Usage

<!-- UsageSnippet language="typescript" operationID="listProjects" method="get" path="/v1/projects" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.projects.list({});

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { projectsList } from "@alienplatform/platform-api/funcs/projectsList.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsList(alien, {});
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("projectsList failed:", res.error);
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
  useProjectsList,
  useProjectsListSuspense,

  // Utility for prefetching data during server-side rendering and in React
  // Server Components that will be immediately available to client components
  // using the hooks.
  prefetchProjectsList,

  // Utilities to invalidate the query cache for this query in response to
  // mutations and other user actions.
  invalidateProjectsList,
  invalidateAllProjectsList,
} from "@alienplatform/platform-api/react-query/projectsList.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.ListProjectsRequest](../../models/operations/listprojectsrequest.md)                                                                                               | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[operations.ListProjectsResponse](../../models/operations/listprojectsresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 400                      | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## create

Create a new project.

### Example Usage

<!-- UsageSnippet language="typescript" operationID="createProject" method="post" path="/v1/projects" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.projects.create({
    name: "my-app",
    gitRepository: {
      type: "github",
      repo: "alien/my-agent",
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
import { projectsCreate } from "@alienplatform/platform-api/funcs/projectsCreate.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsCreate(alien, {
    name: "my-app",
    gitRepository: {
      type: "github",
      repo: "alien/my-agent",
    },
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("projectsCreate failed:", res.error);
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
  useProjectsCreateMutation
} from "@alienplatform/platform-api/react-query/projectsCreate.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.CreateProjectRequest](../../models/operations/createprojectrequest.md)                                                                                             | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[operations.CreateProjectResponse](../../models/operations/createprojectresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 400, 401, 403, 409       | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## get

Retrieve a project by ID or name.

### Example Usage: projectId

<!-- UsageSnippet language="typescript" operationID="getProject" method="get" path="/v1/projects/{idOrName}" example="projectId" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.projects.get({
    idOrName: "prj_mcytp6z3j91f7tn5ryqsfwtr",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { projectsGet } from "@alienplatform/platform-api/funcs/projectsGet.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsGet(alien, {
    idOrName: "prj_mcytp6z3j91f7tn5ryqsfwtr",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("projectsGet failed:", res.error);
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
  useProjectsGet,
  useProjectsGetSuspense,

  // Utility for prefetching data during server-side rendering and in React
  // Server Components that will be immediately available to client components
  // using the hooks.
  prefetchProjectsGet,

  // Utilities to invalidate the query cache for this query in response to
  // mutations and other user actions.
  invalidateProjectsGet,
  invalidateAllProjectsGet,
} from "@alienplatform/platform-api/react-query/projectsGet.js";
```
### Example Usage: projectName

<!-- UsageSnippet language="typescript" operationID="getProject" method="get" path="/v1/projects/{idOrName}" example="projectName" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.projects.get({
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
import { projectsGet } from "@alienplatform/platform-api/funcs/projectsGet.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsGet(alien, {
    idOrName: "my-project",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("projectsGet failed:", res.error);
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
  useProjectsGet,
  useProjectsGetSuspense,

  // Utility for prefetching data during server-side rendering and in React
  // Server Components that will be immediately available to client components
  // using the hooks.
  prefetchProjectsGet,

  // Utilities to invalidate the query cache for this query in response to
  // mutations and other user actions.
  invalidateProjectsGet,
  invalidateAllProjectsGet,
} from "@alienplatform/platform-api/react-query/projectsGet.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.GetProjectRequest](../../models/operations/getprojectrequest.md)                                                                                                   | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.Project](../../models/project.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 404                      | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## update

Update a project.

### Example Usage: projectId

<!-- UsageSnippet language="typescript" operationID="updateProject" method="patch" path="/v1/projects/{idOrName}" example="projectId" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.projects.update("prj_mcytp6z3j91f7tn5ryqsfwtr", {
    gitRepository: null,
    domainId: "dom_469m0agk8luj4s16sakmmpdd",
    defaultManagers: {
      aws: "mgr_enxscjrqiiu2lrc672hwwuc5",
      gcp: "mgr_enxscjrqiiu2lrc672hwwuc5",
      azure: "mgr_enxscjrqiiu2lrc672hwwuc5",
      kubernetes: "mgr_enxscjrqiiu2lrc672hwwuc5",
      machines: "mgr_enxscjrqiiu2lrc672hwwuc5",
      local: "mgr_enxscjrqiiu2lrc672hwwuc5",
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
import { projectsUpdate } from "@alienplatform/platform-api/funcs/projectsUpdate.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsUpdate(alien, "prj_mcytp6z3j91f7tn5ryqsfwtr", {
    gitRepository: null,
    domainId: "dom_469m0agk8luj4s16sakmmpdd",
    defaultManagers: {
      aws: "mgr_enxscjrqiiu2lrc672hwwuc5",
      gcp: "mgr_enxscjrqiiu2lrc672hwwuc5",
      azure: "mgr_enxscjrqiiu2lrc672hwwuc5",
      kubernetes: "mgr_enxscjrqiiu2lrc672hwwuc5",
      machines: "mgr_enxscjrqiiu2lrc672hwwuc5",
      local: "mgr_enxscjrqiiu2lrc672hwwuc5",
    },
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("projectsUpdate failed:", res.error);
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
  useProjectsUpdateMutation
} from "@alienplatform/platform-api/react-query/projectsUpdate.js";
```
### Example Usage: projectName

<!-- UsageSnippet language="typescript" operationID="updateProject" method="patch" path="/v1/projects/{idOrName}" example="projectName" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.projects.update("my-project", {
    gitRepository: null,
    domainId: "dom_469m0agk8luj4s16sakmmpdd",
    defaultManagers: {
      aws: "mgr_enxscjrqiiu2lrc672hwwuc5",
      gcp: "mgr_enxscjrqiiu2lrc672hwwuc5",
      azure: "mgr_enxscjrqiiu2lrc672hwwuc5",
      kubernetes: "mgr_enxscjrqiiu2lrc672hwwuc5",
      machines: "mgr_enxscjrqiiu2lrc672hwwuc5",
      local: "mgr_enxscjrqiiu2lrc672hwwuc5",
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
import { projectsUpdate } from "@alienplatform/platform-api/funcs/projectsUpdate.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsUpdate(alien, "my-project", {
    gitRepository: null,
    domainId: "dom_469m0agk8luj4s16sakmmpdd",
    defaultManagers: {
      aws: "mgr_enxscjrqiiu2lrc672hwwuc5",
      gcp: "mgr_enxscjrqiiu2lrc672hwwuc5",
      azure: "mgr_enxscjrqiiu2lrc672hwwuc5",
      kubernetes: "mgr_enxscjrqiiu2lrc672hwwuc5",
      machines: "mgr_enxscjrqiiu2lrc672hwwuc5",
      local: "mgr_enxscjrqiiu2lrc672hwwuc5",
    },
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("projectsUpdate failed:", res.error);
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
  useProjectsUpdateMutation
} from "@alienplatform/platform-api/react-query/projectsUpdate.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `idOrName`                                                                                                                                                                     | *string*                                                                                                                                                                       | :heavy_check_mark:                                                                                                                                                             | Project ID or name.                                                                                                                                                            |
| `updateProject`                                                                                                                                                                | [models.UpdateProject](../../models/updateproject.md)                                                                                                                          | :heavy_minus_sign:                                                                                                                                                             | N/A                                                                                                                                                                            |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.Project](../../models/project.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 403, 404, 409, 422       | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## delete

Delete a project. The project must have no deployments.

### Example Usage

<!-- UsageSnippet language="typescript" operationID="deleteProject" method="delete" path="/v1/projects/{idOrName}" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  await alien.projects.delete({
    idOrName: "my-project",
  });


}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { projectsDelete } from "@alienplatform/platform-api/funcs/projectsDelete.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsDelete(alien, {
    idOrName: "my-project",
  });
  if (res.ok) {
    const { value: result } = res;

  } else {
    console.log("projectsDelete failed:", res.error);
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
  useProjectsDeleteMutation
} from "@alienplatform/platform-api/react-query/projectsDelete.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.DeleteProjectRequest](../../models/operations/deleteprojectrequest.md)                                                                                             | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<void\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 400, 404                 | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## getGcpOAuthProvider

Retrieve redacted project-level Google Cloud OAuth provider settings.

### Example Usage: projectId

<!-- UsageSnippet language="typescript" operationID="getProjectGcpOAuthProvider" method="get" path="/v1/projects/{idOrName}/gcp-oauth-provider" example="projectId" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.projects.getGcpOAuthProvider({
    idOrName: "prj_mcytp6z3j91f7tn5ryqsfwtr",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { projectsGetGcpOAuthProvider } from "@alienplatform/platform-api/funcs/projectsGetGcpOAuthProvider.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsGetGcpOAuthProvider(alien, {
    idOrName: "prj_mcytp6z3j91f7tn5ryqsfwtr",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("projectsGetGcpOAuthProvider failed:", res.error);
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
  useProjectsGetGcpOAuthProvider,
  useProjectsGetGcpOAuthProviderSuspense,

  // Utility for prefetching data during server-side rendering and in React
  // Server Components that will be immediately available to client components
  // using the hooks.
  prefetchProjectsGetGcpOAuthProvider,

  // Utilities to invalidate the query cache for this query in response to
  // mutations and other user actions.
  invalidateProjectsGetGcpOAuthProvider,
  invalidateAllProjectsGetGcpOAuthProvider,
} from "@alienplatform/platform-api/react-query/projectsGetGcpOAuthProvider.js";
```
### Example Usage: projectName

<!-- UsageSnippet language="typescript" operationID="getProjectGcpOAuthProvider" method="get" path="/v1/projects/{idOrName}/gcp-oauth-provider" example="projectName" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.projects.getGcpOAuthProvider({
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
import { projectsGetGcpOAuthProvider } from "@alienplatform/platform-api/funcs/projectsGetGcpOAuthProvider.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsGetGcpOAuthProvider(alien, {
    idOrName: "my-project",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("projectsGetGcpOAuthProvider failed:", res.error);
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
  useProjectsGetGcpOAuthProvider,
  useProjectsGetGcpOAuthProviderSuspense,

  // Utility for prefetching data during server-side rendering and in React
  // Server Components that will be immediately available to client components
  // using the hooks.
  prefetchProjectsGetGcpOAuthProvider,

  // Utilities to invalidate the query cache for this query in response to
  // mutations and other user actions.
  invalidateProjectsGetGcpOAuthProvider,
  invalidateAllProjectsGetGcpOAuthProvider,
} from "@alienplatform/platform-api/react-query/projectsGetGcpOAuthProvider.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.GetProjectGcpOAuthProviderRequest](../../models/operations/getprojectgcpoauthproviderrequest.md)                                                                   | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.ProjectGcpOAuthProvider](../../models/projectgcpoauthprovider.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 404                      | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## updateGcpOAuthProvider

Update project-level Google Cloud OAuth provider settings.

### Example Usage: projectId

<!-- UsageSnippet language="typescript" operationID="updateProjectGcpOAuthProvider" method="put" path="/v1/projects/{idOrName}/gcp-oauth-provider" example="projectId" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.projects.updateGcpOAuthProvider("prj_mcytp6z3j91f7tn5ryqsfwtr", {
    mode: "alien-managed",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { projectsUpdateGcpOAuthProvider } from "@alienplatform/platform-api/funcs/projectsUpdateGcpOAuthProvider.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsUpdateGcpOAuthProvider(alien, "prj_mcytp6z3j91f7tn5ryqsfwtr", {
    mode: "alien-managed",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("projectsUpdateGcpOAuthProvider failed:", res.error);
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
  useProjectsUpdateGcpOAuthProviderMutation
} from "@alienplatform/platform-api/react-query/projectsUpdateGcpOAuthProvider.js";
```
### Example Usage: projectName

<!-- UsageSnippet language="typescript" operationID="updateProjectGcpOAuthProvider" method="put" path="/v1/projects/{idOrName}/gcp-oauth-provider" example="projectName" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.projects.updateGcpOAuthProvider("my-project", {
    mode: "custom",
    clientId: "1234567890-abc123.apps.googleusercontent.com",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { projectsUpdateGcpOAuthProvider } from "@alienplatform/platform-api/funcs/projectsUpdateGcpOAuthProvider.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsUpdateGcpOAuthProvider(alien, "my-project", {
    mode: "custom",
    clientId: "1234567890-abc123.apps.googleusercontent.com",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("projectsUpdateGcpOAuthProvider failed:", res.error);
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
  useProjectsUpdateGcpOAuthProviderMutation
} from "@alienplatform/platform-api/react-query/projectsUpdateGcpOAuthProvider.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `idOrName`                                                                                                                                                                     | *string*                                                                                                                                                                       | :heavy_check_mark:                                                                                                                                                             | Project ID or name.                                                                                                                                                            |
| `updateProjectGcpOAuthProvider`                                                                                                                                                | *models.UpdateProjectGcpOAuthProvider*                                                                                                                                         | :heavy_minus_sign:                                                                                                                                                             | N/A                                                                                                                                                                            |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.ProjectGcpOAuthProvider](../../models/projectgcpoauthprovider.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 400, 404                 | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## configureSource

Connect a GitHub repository or Alien template to an existing project.

### Example Usage: projectId

<!-- UsageSnippet language="typescript" operationID="configureProjectSource" method="post" path="/v1/projects/{idOrName}/source" example="projectId" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.projects.configureSource("prj_mcytp6z3j91f7tn5ryqsfwtr", {
    mode: "template",
    targetNamespace: "<value>",
    templatePath: "examples/github-agent/packages/remote-agent",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { projectsConfigureSource } from "@alienplatform/platform-api/funcs/projectsConfigureSource.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsConfigureSource(alien, "prj_mcytp6z3j91f7tn5ryqsfwtr", {
    mode: "template",
    targetNamespace: "<value>",
    templatePath: "examples/github-agent/packages/remote-agent",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("projectsConfigureSource failed:", res.error);
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
  useProjectsConfigureSourceMutation
} from "@alienplatform/platform-api/react-query/projectsConfigureSource.js";
```
### Example Usage: projectName

<!-- UsageSnippet language="typescript" operationID="configureProjectSource" method="post" path="/v1/projects/{idOrName}/source" example="projectName" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.projects.configureSource("my-project", {
    mode: "repository",
    gitRepository: {
      type: "github",
      repo: "alien/my-agent",
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
import { projectsConfigureSource } from "@alienplatform/platform-api/funcs/projectsConfigureSource.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsConfigureSource(alien, "my-project", {
    mode: "repository",
    gitRepository: {
      type: "github",
      repo: "alien/my-agent",
    },
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("projectsConfigureSource failed:", res.error);
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
  useProjectsConfigureSourceMutation
} from "@alienplatform/platform-api/react-query/projectsConfigureSource.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `idOrName`                                                                                                                                                                     | *string*                                                                                                                                                                       | :heavy_check_mark:                                                                                                                                                             | Project ID or name.                                                                                                                                                            |
| `requestBody`                                                                                                                                                                  | *operations.ConfigureProjectSourceRequestBody*                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | N/A                                                                                                                                                                            |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[operations.ConfigureProjectSourceResponse](../../models/operations/configureprojectsourceresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 400, 403, 404            | application/json         |
| errors.APIError          | 500, 503                 | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## getDeploymentPortalDomain

Get the deployment portal domain binding for a project.

### Example Usage: projectId

<!-- UsageSnippet language="typescript" operationID="getProjectDeploymentPortalDomain" method="get" path="/v1/projects/{idOrName}/deployment-portal-domain" example="projectId" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.projects.getDeploymentPortalDomain({
    idOrName: "prj_mcytp6z3j91f7tn5ryqsfwtr",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { projectsGetDeploymentPortalDomain } from "@alienplatform/platform-api/funcs/projectsGetDeploymentPortalDomain.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsGetDeploymentPortalDomain(alien, {
    idOrName: "prj_mcytp6z3j91f7tn5ryqsfwtr",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("projectsGetDeploymentPortalDomain failed:", res.error);
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
  useProjectsGetDeploymentPortalDomain,
  useProjectsGetDeploymentPortalDomainSuspense,

  // Utility for prefetching data during server-side rendering and in React
  // Server Components that will be immediately available to client components
  // using the hooks.
  prefetchProjectsGetDeploymentPortalDomain,

  // Utilities to invalidate the query cache for this query in response to
  // mutations and other user actions.
  invalidateProjectsGetDeploymentPortalDomain,
  invalidateAllProjectsGetDeploymentPortalDomain,
} from "@alienplatform/platform-api/react-query/projectsGetDeploymentPortalDomain.js";
```
### Example Usage: projectName

<!-- UsageSnippet language="typescript" operationID="getProjectDeploymentPortalDomain" method="get" path="/v1/projects/{idOrName}/deployment-portal-domain" example="projectName" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.projects.getDeploymentPortalDomain({
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
import { projectsGetDeploymentPortalDomain } from "@alienplatform/platform-api/funcs/projectsGetDeploymentPortalDomain.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsGetDeploymentPortalDomain(alien, {
    idOrName: "my-project",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("projectsGetDeploymentPortalDomain failed:", res.error);
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
  useProjectsGetDeploymentPortalDomain,
  useProjectsGetDeploymentPortalDomainSuspense,

  // Utility for prefetching data during server-side rendering and in React
  // Server Components that will be immediately available to client components
  // using the hooks.
  prefetchProjectsGetDeploymentPortalDomain,

  // Utilities to invalidate the query cache for this query in response to
  // mutations and other user actions.
  invalidateProjectsGetDeploymentPortalDomain,
  invalidateAllProjectsGetDeploymentPortalDomain,
} from "@alienplatform/platform-api/react-query/projectsGetDeploymentPortalDomain.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.GetProjectDeploymentPortalDomainRequest](../../models/operations/getprojectdeploymentportaldomainrequest.md)                                                       | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.DeploymentPortalDomainResponse](../../models/deploymentportaldomainresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 404                      | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## createFromTemplate

Create a project by forking alienplatform/alien into your namespace.

### Example Usage

<!-- UsageSnippet language="typescript" operationID="createProjectFromTemplate" method="post" path="/v1/projects/import-template" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.projects.createFromTemplate({
    name: "my-app",
    targetNamespace: "<value>",
    templatePath: "examples/remote-worker-ts",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { projectsCreateFromTemplate } from "@alienplatform/platform-api/funcs/projectsCreateFromTemplate.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsCreateFromTemplate(alien, {
    name: "my-app",
    targetNamespace: "<value>",
    templatePath: "examples/remote-worker-ts",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("projectsCreateFromTemplate failed:", res.error);
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
  useProjectsCreateFromTemplateMutation
} from "@alienplatform/platform-api/react-query/projectsCreateFromTemplate.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.CreateProjectFromTemplateRequest](../../models/operations/createprojectfromtemplaterequest.md)                                                                     | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[operations.CreateProjectFromTemplateResponse](../../models/operations/createprojectfromtemplateresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 400, 409                 | application/json         |
| errors.APIError          | 500, 503                 | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## getTemplateUrls

Get template URLs for deploying setup stacks in this project.

### Example Usage: projectId

<!-- UsageSnippet language="typescript" operationID="getProjectTemplateUrls" method="get" path="/v1/projects/{idOrName}/template-urls" example="projectId" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.projects.getTemplateUrls({
    idOrName: "prj_mcytp6z3j91f7tn5ryqsfwtr",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { projectsGetTemplateUrls } from "@alienplatform/platform-api/funcs/projectsGetTemplateUrls.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsGetTemplateUrls(alien, {
    idOrName: "prj_mcytp6z3j91f7tn5ryqsfwtr",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("projectsGetTemplateUrls failed:", res.error);
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
  useProjectsGetTemplateUrls,
  useProjectsGetTemplateUrlsSuspense,

  // Utility for prefetching data during server-side rendering and in React
  // Server Components that will be immediately available to client components
  // using the hooks.
  prefetchProjectsGetTemplateUrls,

  // Utilities to invalidate the query cache for this query in response to
  // mutations and other user actions.
  invalidateProjectsGetTemplateUrls,
  invalidateAllProjectsGetTemplateUrls,
} from "@alienplatform/platform-api/react-query/projectsGetTemplateUrls.js";
```
### Example Usage: projectName

<!-- UsageSnippet language="typescript" operationID="getProjectTemplateUrls" method="get" path="/v1/projects/{idOrName}/template-urls" example="projectName" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.projects.getTemplateUrls({
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
import { projectsGetTemplateUrls } from "@alienplatform/platform-api/funcs/projectsGetTemplateUrls.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsGetTemplateUrls(alien, {
    idOrName: "my-project",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("projectsGetTemplateUrls failed:", res.error);
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
  useProjectsGetTemplateUrls,
  useProjectsGetTemplateUrlsSuspense,

  // Utility for prefetching data during server-side rendering and in React
  // Server Components that will be immediately available to client components
  // using the hooks.
  prefetchProjectsGetTemplateUrls,

  // Utilities to invalidate the query cache for this query in response to
  // mutations and other user actions.
  invalidateProjectsGetTemplateUrls,
  invalidateAllProjectsGetTemplateUrls,
} from "@alienplatform/platform-api/react-query/projectsGetTemplateUrls.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.GetProjectTemplateUrlsRequest](../../models/operations/getprojecttemplateurlsrequest.md)                                                                           | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[operations.GetProjectTemplateUrlsResponse](../../models/operations/getprojecttemplateurlsresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 404                      | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## getDeploymentLinkSetup

Get the active release stack and portal-visible setup availability for deployment-link configuration.

### Example Usage: projectId

<!-- UsageSnippet language="typescript" operationID="getProjectDeploymentLinkSetup" method="get" path="/v1/projects/{idOrName}/deployment-link-setup" example="projectId" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.projects.getDeploymentLinkSetup({
    idOrName: "prj_mcytp6z3j91f7tn5ryqsfwtr",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { projectsGetDeploymentLinkSetup } from "@alienplatform/platform-api/funcs/projectsGetDeploymentLinkSetup.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsGetDeploymentLinkSetup(alien, {
    idOrName: "prj_mcytp6z3j91f7tn5ryqsfwtr",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("projectsGetDeploymentLinkSetup failed:", res.error);
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
  useProjectsGetDeploymentLinkSetup,
  useProjectsGetDeploymentLinkSetupSuspense,

  // Utility for prefetching data during server-side rendering and in React
  // Server Components that will be immediately available to client components
  // using the hooks.
  prefetchProjectsGetDeploymentLinkSetup,

  // Utilities to invalidate the query cache for this query in response to
  // mutations and other user actions.
  invalidateProjectsGetDeploymentLinkSetup,
  invalidateAllProjectsGetDeploymentLinkSetup,
} from "@alienplatform/platform-api/react-query/projectsGetDeploymentLinkSetup.js";
```
### Example Usage: projectName

<!-- UsageSnippet language="typescript" operationID="getProjectDeploymentLinkSetup" method="get" path="/v1/projects/{idOrName}/deployment-link-setup" example="projectName" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.projects.getDeploymentLinkSetup({
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
import { projectsGetDeploymentLinkSetup } from "@alienplatform/platform-api/funcs/projectsGetDeploymentLinkSetup.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsGetDeploymentLinkSetup(alien, {
    idOrName: "my-project",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("projectsGetDeploymentLinkSetup failed:", res.error);
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
  useProjectsGetDeploymentLinkSetup,
  useProjectsGetDeploymentLinkSetupSuspense,

  // Utility for prefetching data during server-side rendering and in React
  // Server Components that will be immediately available to client components
  // using the hooks.
  prefetchProjectsGetDeploymentLinkSetup,

  // Utilities to invalidate the query cache for this query in response to
  // mutations and other user actions.
  invalidateProjectsGetDeploymentLinkSetup,
  invalidateAllProjectsGetDeploymentLinkSetup,
} from "@alienplatform/platform-api/react-query/projectsGetDeploymentLinkSetup.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.GetProjectDeploymentLinkSetupRequest](../../models/operations/getprojectdeploymentlinksetuprequest.md)                                                             | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.DeploymentLinkSetupResponse](../../models/deploymentlinksetupresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 400, 404                 | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## getActiveRelease

Get the production channel's current release. When deploymentId is provided, returns that deployment's effective release: its pin, or its followed channel's current release.

### Example Usage: projectId

<!-- UsageSnippet language="typescript" operationID="getProjectActiveRelease" method="get" path="/v1/projects/{idOrName}/active-release" example="projectId" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.projects.getActiveRelease({
    idOrName: "prj_mcytp6z3j91f7tn5ryqsfwtr",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { projectsGetActiveRelease } from "@alienplatform/platform-api/funcs/projectsGetActiveRelease.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsGetActiveRelease(alien, {
    idOrName: "prj_mcytp6z3j91f7tn5ryqsfwtr",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("projectsGetActiveRelease failed:", res.error);
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
  useProjectsGetActiveRelease,
  useProjectsGetActiveReleaseSuspense,

  // Utility for prefetching data during server-side rendering and in React
  // Server Components that will be immediately available to client components
  // using the hooks.
  prefetchProjectsGetActiveRelease,

  // Utilities to invalidate the query cache for this query in response to
  // mutations and other user actions.
  invalidateProjectsGetActiveRelease,
  invalidateAllProjectsGetActiveRelease,
} from "@alienplatform/platform-api/react-query/projectsGetActiveRelease.js";
```
### Example Usage: projectName

<!-- UsageSnippet language="typescript" operationID="getProjectActiveRelease" method="get" path="/v1/projects/{idOrName}/active-release" example="projectName" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.projects.getActiveRelease({
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
import { projectsGetActiveRelease } from "@alienplatform/platform-api/funcs/projectsGetActiveRelease.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsGetActiveRelease(alien, {
    idOrName: "my-project",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("projectsGetActiveRelease failed:", res.error);
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
  useProjectsGetActiveRelease,
  useProjectsGetActiveReleaseSuspense,

  // Utility for prefetching data during server-side rendering and in React
  // Server Components that will be immediately available to client components
  // using the hooks.
  prefetchProjectsGetActiveRelease,

  // Utilities to invalidate the query cache for this query in response to
  // mutations and other user actions.
  invalidateProjectsGetActiveRelease,
  invalidateAllProjectsGetActiveRelease,
} from "@alienplatform/platform-api/react-query/projectsGetActiveRelease.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.GetProjectActiveReleaseRequest](../../models/operations/getprojectactivereleaserequest.md)                                                                         | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[any](../../models/.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 404                      | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## previewModelsImpact

Preview which customer model connections a configuration change may affect.

### Example Usage: projectId

<!-- UsageSnippet language="typescript" operationID="previewProjectModelsImpact" method="post" path="/v1/projects/{idOrName}/project-capabilities/models/impact" example="projectId" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.projects.previewModelsImpact("prj_mcytp6z3j91f7tn5ryqsfwtr");

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { projectsPreviewModelsImpact } from "@alienplatform/platform-api/funcs/projectsPreviewModelsImpact.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsPreviewModelsImpact(alien, "prj_mcytp6z3j91f7tn5ryqsfwtr");
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("projectsPreviewModelsImpact failed:", res.error);
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
  useProjectsPreviewModelsImpactMutation
} from "@alienplatform/platform-api/react-query/projectsPreviewModelsImpact.js";
```
### Example Usage: projectName

<!-- UsageSnippet language="typescript" operationID="previewProjectModelsImpact" method="post" path="/v1/projects/{idOrName}/project-capabilities/models/impact" example="projectName" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.projects.previewModelsImpact("my-project");

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { projectsPreviewModelsImpact } from "@alienplatform/platform-api/funcs/projectsPreviewModelsImpact.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsPreviewModelsImpact(alien, "my-project");
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("projectsPreviewModelsImpact failed:", res.error);
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
  useProjectsPreviewModelsImpactMutation
} from "@alienplatform/platform-api/react-query/projectsPreviewModelsImpact.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `idOrName`                                                                                                                                                                     | *string*                                                                                                                                                                       | :heavy_check_mark:                                                                                                                                                             | Project ID or name.                                                                                                                                                            |
| `configureModelsRequest`                                                                                                                                                       | [models.ConfigureModelsRequest](../../models/configuremodelsrequest.md)                                                                                                        | :heavy_minus_sign:                                                                                                                                                             | N/A                                                                                                                                                                            |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[operations.PreviewProjectModelsImpactResponse](../../models/operations/previewprojectmodelsimpactresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 403, 404                 | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## setCapabilities

Set the capabilities offered by a Project. Removing a capability prevents new setup without deleting existing customer resources.

### Example Usage: projectId

<!-- UsageSnippet language="typescript" operationID="setProjectCapabilities" method="put" path="/v1/projects/{idOrName}/project-capabilities" example="projectId" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.projects.setCapabilities("prj_mcytp6z3j91f7tn5ryqsfwtr");

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { projectsSetCapabilities } from "@alienplatform/platform-api/funcs/projectsSetCapabilities.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsSetCapabilities(alien, "prj_mcytp6z3j91f7tn5ryqsfwtr");
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("projectsSetCapabilities failed:", res.error);
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
  useProjectsSetCapabilitiesMutation
} from "@alienplatform/platform-api/react-query/projectsSetCapabilities.js";
```
### Example Usage: projectName

<!-- UsageSnippet language="typescript" operationID="setProjectCapabilities" method="put" path="/v1/projects/{idOrName}/project-capabilities" example="projectName" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.projects.setCapabilities("my-project");

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { projectsSetCapabilities } from "@alienplatform/platform-api/funcs/projectsSetCapabilities.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsSetCapabilities(alien, "my-project");
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("projectsSetCapabilities failed:", res.error);
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
  useProjectsSetCapabilitiesMutation
} from "@alienplatform/platform-api/react-query/projectsSetCapabilities.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `idOrName`                                                                                                                                                                     | *string*                                                                                                                                                                       | :heavy_check_mark:                                                                                                                                                             | Project ID or name.                                                                                                                                                            |
| `requestBody`                                                                                                                                                                  | [operations.SetProjectCapabilitiesRequestBody](../../models/operations/setprojectcapabilitiesrequestbody.md)                                                                   | :heavy_minus_sign:                                                                                                                                                             | N/A                                                                                                                                                                            |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.ProjectCapabilities](../../models/projectcapabilities.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 403, 404, 409            | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## configureDeployments

Enable deployments for a Project.

### Example Usage: projectId

<!-- UsageSnippet language="typescript" operationID="configureProjectDeployments" method="put" path="/v1/projects/{idOrName}/project-capabilities/deployments" example="projectId" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.projects.configureDeployments("prj_mcytp6z3j91f7tn5ryqsfwtr");

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { projectsConfigureDeployments } from "@alienplatform/platform-api/funcs/projectsConfigureDeployments.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsConfigureDeployments(alien, "prj_mcytp6z3j91f7tn5ryqsfwtr");
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("projectsConfigureDeployments failed:", res.error);
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
  useProjectsConfigureDeploymentsMutation
} from "@alienplatform/platform-api/react-query/projectsConfigureDeployments.js";
```
### Example Usage: projectName

<!-- UsageSnippet language="typescript" operationID="configureProjectDeployments" method="put" path="/v1/projects/{idOrName}/project-capabilities/deployments" example="projectName" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.projects.configureDeployments("my-project");

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { projectsConfigureDeployments } from "@alienplatform/platform-api/funcs/projectsConfigureDeployments.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsConfigureDeployments(alien, "my-project");
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("projectsConfigureDeployments failed:", res.error);
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
  useProjectsConfigureDeploymentsMutation
} from "@alienplatform/platform-api/react-query/projectsConfigureDeployments.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `idOrName`                                                                                                                                                                     | *string*                                                                                                                                                                       | :heavy_check_mark:                                                                                                                                                             | Project ID or name.                                                                                                                                                            |
| `requestBody`                                                                                                                                                                  | [operations.ConfigureProjectDeploymentsRequestBody](../../models/operations/configureprojectdeploymentsrequestbody.md)                                                         | :heavy_minus_sign:                                                                                                                                                             | N/A                                                                                                                                                                            |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.ProjectCapabilities](../../models/projectcapabilities.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 403, 404, 409            | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## getAiProviderHeaders

Get static headers added to AI requests for each provider.

### Example Usage: projectId

<!-- UsageSnippet language="typescript" operationID="getProjectAiProviderHeaders" method="get" path="/v1/projects/{idOrName}/ai-provider-headers" example="projectId" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.projects.getAiProviderHeaders({
    idOrName: "prj_mcytp6z3j91f7tn5ryqsfwtr",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { projectsGetAiProviderHeaders } from "@alienplatform/platform-api/funcs/projectsGetAiProviderHeaders.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsGetAiProviderHeaders(alien, {
    idOrName: "prj_mcytp6z3j91f7tn5ryqsfwtr",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("projectsGetAiProviderHeaders failed:", res.error);
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
  useProjectsGetAiProviderHeaders,
  useProjectsGetAiProviderHeadersSuspense,

  // Utility for prefetching data during server-side rendering and in React
  // Server Components that will be immediately available to client components
  // using the hooks.
  prefetchProjectsGetAiProviderHeaders,

  // Utilities to invalidate the query cache for this query in response to
  // mutations and other user actions.
  invalidateProjectsGetAiProviderHeaders,
  invalidateAllProjectsGetAiProviderHeaders,
} from "@alienplatform/platform-api/react-query/projectsGetAiProviderHeaders.js";
```
### Example Usage: projectName

<!-- UsageSnippet language="typescript" operationID="getProjectAiProviderHeaders" method="get" path="/v1/projects/{idOrName}/ai-provider-headers" example="projectName" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.projects.getAiProviderHeaders({
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
import { projectsGetAiProviderHeaders } from "@alienplatform/platform-api/funcs/projectsGetAiProviderHeaders.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsGetAiProviderHeaders(alien, {
    idOrName: "my-project",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("projectsGetAiProviderHeaders failed:", res.error);
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
  useProjectsGetAiProviderHeaders,
  useProjectsGetAiProviderHeadersSuspense,

  // Utility for prefetching data during server-side rendering and in React
  // Server Components that will be immediately available to client components
  // using the hooks.
  prefetchProjectsGetAiProviderHeaders,

  // Utilities to invalidate the query cache for this query in response to
  // mutations and other user actions.
  invalidateProjectsGetAiProviderHeaders,
  invalidateAllProjectsGetAiProviderHeaders,
} from "@alienplatform/platform-api/react-query/projectsGetAiProviderHeaders.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.GetProjectAiProviderHeadersRequest](../../models/operations/getprojectaiproviderheadersrequest.md)                                                                 | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.AIProviderHeaders](../../models/aiproviderheaders.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 403, 404                 | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## configureAiProviderHeaders

Replace the static headers added to AI requests for each provider.

### Example Usage: projectId

<!-- UsageSnippet language="typescript" operationID="configureProjectAiProviderHeaders" method="put" path="/v1/projects/{idOrName}/ai-provider-headers" example="projectId" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.projects.configureAiProviderHeaders("prj_mcytp6z3j91f7tn5ryqsfwtr");

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { projectsConfigureAiProviderHeaders } from "@alienplatform/platform-api/funcs/projectsConfigureAiProviderHeaders.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsConfigureAiProviderHeaders(alien, "prj_mcytp6z3j91f7tn5ryqsfwtr");
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("projectsConfigureAiProviderHeaders failed:", res.error);
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
  useProjectsConfigureAiProviderHeadersMutation
} from "@alienplatform/platform-api/react-query/projectsConfigureAiProviderHeaders.js";
```
### Example Usage: projectName

<!-- UsageSnippet language="typescript" operationID="configureProjectAiProviderHeaders" method="put" path="/v1/projects/{idOrName}/ai-provider-headers" example="projectName" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.projects.configureAiProviderHeaders("my-project");

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { projectsConfigureAiProviderHeaders } from "@alienplatform/platform-api/funcs/projectsConfigureAiProviderHeaders.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsConfigureAiProviderHeaders(alien, "my-project");
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("projectsConfigureAiProviderHeaders failed:", res.error);
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
  useProjectsConfigureAiProviderHeadersMutation
} from "@alienplatform/platform-api/react-query/projectsConfigureAiProviderHeaders.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `idOrName`                                                                                                                                                                     | *string*                                                                                                                                                                       | :heavy_check_mark:                                                                                                                                                             | Project ID or name.                                                                                                                                                            |
| `aiProviderHeaders`                                                                                                                                                            | [models.AIProviderHeaders](../../models/aiproviderheaders.md)                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | N/A                                                                                                                                                                            |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.AIProviderHeaders](../../models/aiproviderheaders.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 400, 403, 404            | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## configureModels

Configure customer-owned model providers without requiring an application Release.

### Example Usage: projectId

<!-- UsageSnippet language="typescript" operationID="configureProjectModels" method="put" path="/v1/projects/{idOrName}/project-capabilities/models" example="projectId" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.projects.configureModels("prj_mcytp6z3j91f7tn5ryqsfwtr");

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { projectsConfigureModels } from "@alienplatform/platform-api/funcs/projectsConfigureModels.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsConfigureModels(alien, "prj_mcytp6z3j91f7tn5ryqsfwtr");
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("projectsConfigureModels failed:", res.error);
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
  useProjectsConfigureModelsMutation
} from "@alienplatform/platform-api/react-query/projectsConfigureModels.js";
```
### Example Usage: projectName

<!-- UsageSnippet language="typescript" operationID="configureProjectModels" method="put" path="/v1/projects/{idOrName}/project-capabilities/models" example="projectName" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.projects.configureModels("my-project");

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { projectsConfigureModels } from "@alienplatform/platform-api/funcs/projectsConfigureModels.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsConfigureModels(alien, "my-project");
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("projectsConfigureModels failed:", res.error);
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
  useProjectsConfigureModelsMutation
} from "@alienplatform/platform-api/react-query/projectsConfigureModels.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `idOrName`                                                                                                                                                                     | *string*                                                                                                                                                                       | :heavy_check_mark:                                                                                                                                                             | Project ID or name.                                                                                                                                                            |
| `configureModelsRequest`                                                                                                                                                       | [models.ConfigureModelsRequest](../../models/configuremodelsrequest.md)                                                                                                        | :heavy_minus_sign:                                                                                                                                                             | N/A                                                                                                                                                                            |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.CapabilityMaterialization](../../models/capabilitymaterialization.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 403, 404, 409            | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## configureKeys

Enable customer-owned application encryption without requiring an application Release.

### Example Usage: projectId

<!-- UsageSnippet language="typescript" operationID="configureProjectKeys" method="put" path="/v1/projects/{idOrName}/project-capabilities/keys" example="projectId" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.projects.configureKeys("prj_mcytp6z3j91f7tn5ryqsfwtr");

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { projectsConfigureKeys } from "@alienplatform/platform-api/funcs/projectsConfigureKeys.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsConfigureKeys(alien, "prj_mcytp6z3j91f7tn5ryqsfwtr");
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("projectsConfigureKeys failed:", res.error);
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
  useProjectsConfigureKeysMutation
} from "@alienplatform/platform-api/react-query/projectsConfigureKeys.js";
```
### Example Usage: projectName

<!-- UsageSnippet language="typescript" operationID="configureProjectKeys" method="put" path="/v1/projects/{idOrName}/project-capabilities/keys" example="projectName" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.projects.configureKeys("my-project");

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { projectsConfigureKeys } from "@alienplatform/platform-api/funcs/projectsConfigureKeys.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsConfigureKeys(alien, "my-project");
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("projectsConfigureKeys failed:", res.error);
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
  useProjectsConfigureKeysMutation
} from "@alienplatform/platform-api/react-query/projectsConfigureKeys.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `idOrName`                                                                                                                                                                     | *string*                                                                                                                                                                       | :heavy_check_mark:                                                                                                                                                             | Project ID or name.                                                                                                                                                            |
| `requestBody`                                                                                                                                                                  | [operations.ConfigureProjectKeysRequestBody](../../models/operations/configureprojectkeysrequestbody.md)                                                                       | :heavy_minus_sign:                                                                                                                                                             | N/A                                                                                                                                                                            |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.CapabilityMaterialization](../../models/capabilitymaterialization.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 403, 404, 409            | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## configureBuckets

Enable buckets without requiring a project Release.

### Example Usage: projectId

<!-- UsageSnippet language="typescript" operationID="configureProjectBuckets" method="put" path="/v1/projects/{idOrName}/project-capabilities/buckets" example="projectId" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.projects.configureBuckets("prj_mcytp6z3j91f7tn5ryqsfwtr");

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { projectsConfigureBuckets } from "@alienplatform/platform-api/funcs/projectsConfigureBuckets.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsConfigureBuckets(alien, "prj_mcytp6z3j91f7tn5ryqsfwtr");
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("projectsConfigureBuckets failed:", res.error);
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
  useProjectsConfigureBucketsMutation
} from "@alienplatform/platform-api/react-query/projectsConfigureBuckets.js";
```
### Example Usage: projectName

<!-- UsageSnippet language="typescript" operationID="configureProjectBuckets" method="put" path="/v1/projects/{idOrName}/project-capabilities/buckets" example="projectName" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.projects.configureBuckets("my-project");

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { projectsConfigureBuckets } from "@alienplatform/platform-api/funcs/projectsConfigureBuckets.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsConfigureBuckets(alien, "my-project");
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("projectsConfigureBuckets failed:", res.error);
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
  useProjectsConfigureBucketsMutation
} from "@alienplatform/platform-api/react-query/projectsConfigureBuckets.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `idOrName`                                                                                                                                                                     | *string*                                                                                                                                                                       | :heavy_check_mark:                                                                                                                                                             | Project ID or name.                                                                                                                                                            |
| `requestBody`                                                                                                                                                                  | [operations.ConfigureProjectBucketsRequestBody](../../models/operations/configureprojectbucketsrequestbody.md)                                                                 | :heavy_minus_sign:                                                                                                                                                             | N/A                                                                                                                                                                            |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.CapabilityMaterialization](../../models/capabilitymaterialization.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 403, 404, 409            | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## configureRegistry

Enable customer-owned container registries without requiring an application Release.

### Example Usage: projectId

<!-- UsageSnippet language="typescript" operationID="configureProjectRegistry" method="put" path="/v1/projects/{idOrName}/project-capabilities/registry" example="projectId" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.projects.configureRegistry("prj_mcytp6z3j91f7tn5ryqsfwtr");

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { projectsConfigureRegistry } from "@alienplatform/platform-api/funcs/projectsConfigureRegistry.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsConfigureRegistry(alien, "prj_mcytp6z3j91f7tn5ryqsfwtr");
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("projectsConfigureRegistry failed:", res.error);
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
  useProjectsConfigureRegistryMutation
} from "@alienplatform/platform-api/react-query/projectsConfigureRegistry.js";
```
### Example Usage: projectName

<!-- UsageSnippet language="typescript" operationID="configureProjectRegistry" method="put" path="/v1/projects/{idOrName}/project-capabilities/registry" example="projectName" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.projects.configureRegistry("my-project");

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { projectsConfigureRegistry } from "@alienplatform/platform-api/funcs/projectsConfigureRegistry.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsConfigureRegistry(alien, "my-project");
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("projectsConfigureRegistry failed:", res.error);
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
  useProjectsConfigureRegistryMutation
} from "@alienplatform/platform-api/react-query/projectsConfigureRegistry.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `idOrName`                                                                                                                                                                     | *string*                                                                                                                                                                       | :heavy_check_mark:                                                                                                                                                             | Project ID or name.                                                                                                                                                            |
| `requestBody`                                                                                                                                                                  | [operations.ConfigureProjectRegistryRequestBody](../../models/operations/configureprojectregistryrequestbody.md)                                                               | :heavy_minus_sign:                                                                                                                                                             | N/A                                                                                                                                                                            |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.CapabilityMaterialization](../../models/capabilitymaterialization.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 403, 404, 409            | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## configureRemoteSandbox

Enable a customer-owned sandbox a hosted caller can drive through Remote Bindings. Without a custom image it publishes to every cloud on Alien's default image, which the project then follows. A custom image publishes to AWS; a public one with a linux/amd64 variant also publishes to Azure, and to GCP once Alien has built its GCP image.

### Example Usage: projectId

<!-- UsageSnippet language="typescript" operationID="configureProjectRemoteSandbox" method="put" path="/v1/projects/{idOrName}/project-capabilities/remote-sandbox" example="projectId" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.projects.configureRemoteSandbox("prj_mcytp6z3j91f7tn5ryqsfwtr");

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { projectsConfigureRemoteSandbox } from "@alienplatform/platform-api/funcs/projectsConfigureRemoteSandbox.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsConfigureRemoteSandbox(alien, "prj_mcytp6z3j91f7tn5ryqsfwtr");
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("projectsConfigureRemoteSandbox failed:", res.error);
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
  useProjectsConfigureRemoteSandboxMutation
} from "@alienplatform/platform-api/react-query/projectsConfigureRemoteSandbox.js";
```
### Example Usage: projectName

<!-- UsageSnippet language="typescript" operationID="configureProjectRemoteSandbox" method="put" path="/v1/projects/{idOrName}/project-capabilities/remote-sandbox" example="projectName" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.projects.configureRemoteSandbox("my-project");

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { projectsConfigureRemoteSandbox } from "@alienplatform/platform-api/funcs/projectsConfigureRemoteSandbox.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsConfigureRemoteSandbox(alien, "my-project");
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("projectsConfigureRemoteSandbox failed:", res.error);
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
  useProjectsConfigureRemoteSandboxMutation
} from "@alienplatform/platform-api/react-query/projectsConfigureRemoteSandbox.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `idOrName`                                                                                                                                                                     | *string*                                                                                                                                                                       | :heavy_check_mark:                                                                                                                                                             | Project ID or name.                                                                                                                                                            |
| `configureRemoteSandboxRequest`                                                                                                                                                | [models.ConfigureRemoteSandboxRequest](../../models/configureremotesandboxrequest.md)                                                                                          | :heavy_minus_sign:                                                                                                                                                             | N/A                                                                                                                                                                            |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.CapabilityMaterialization](../../models/capabilitymaterialization.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 400, 403, 404, 409, 422  | application/json         |
| errors.APIError          | 500, 503                 | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## ensureSandboxBaseImageRepository

Ensure the project's private image repository exists and return where to push a private sandbox base image. Name the pushed image as the remote sandbox base image afterwards.

### Example Usage: projectId

<!-- UsageSnippet language="typescript" operationID="ensureProjectSandboxBaseImageRepository" method="post" path="/v1/projects/{idOrName}/project-capabilities/remote-sandbox/base-image-repository" example="projectId" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.projects.ensureSandboxBaseImageRepository({
    idOrName: "prj_mcytp6z3j91f7tn5ryqsfwtr",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { projectsEnsureSandboxBaseImageRepository } from "@alienplatform/platform-api/funcs/projectsEnsureSandboxBaseImageRepository.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsEnsureSandboxBaseImageRepository(alien, {
    idOrName: "prj_mcytp6z3j91f7tn5ryqsfwtr",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("projectsEnsureSandboxBaseImageRepository failed:", res.error);
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
  useProjectsEnsureSandboxBaseImageRepositoryMutation
} from "@alienplatform/platform-api/react-query/projectsEnsureSandboxBaseImageRepository.js";
```
### Example Usage: projectName

<!-- UsageSnippet language="typescript" operationID="ensureProjectSandboxBaseImageRepository" method="post" path="/v1/projects/{idOrName}/project-capabilities/remote-sandbox/base-image-repository" example="projectName" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.projects.ensureSandboxBaseImageRepository({
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
import { projectsEnsureSandboxBaseImageRepository } from "@alienplatform/platform-api/funcs/projectsEnsureSandboxBaseImageRepository.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsEnsureSandboxBaseImageRepository(alien, {
    idOrName: "my-project",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("projectsEnsureSandboxBaseImageRepository failed:", res.error);
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
  useProjectsEnsureSandboxBaseImageRepositoryMutation
} from "@alienplatform/platform-api/react-query/projectsEnsureSandboxBaseImageRepository.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.EnsureProjectSandboxBaseImageRepositoryRequest](../../models/operations/ensureprojectsandboxbaseimagerepositoryrequest.md)                                         | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.SandboxBaseImageRepository](../../models/sandboxbaseimagerepository.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 403, 404                 | application/json         |
| errors.APIError          | 500, 502, 503            | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## getCapabilityOverview

Get safe, server-derived capability status for a Project.

### Example Usage: projectId

<!-- UsageSnippet language="typescript" operationID="getProjectCapabilityOverview" method="get" path="/v1/projects/{idOrName}/project-capabilities/overview" example="projectId" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.projects.getCapabilityOverview({
    idOrName: "prj_mcytp6z3j91f7tn5ryqsfwtr",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { projectsGetCapabilityOverview } from "@alienplatform/platform-api/funcs/projectsGetCapabilityOverview.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsGetCapabilityOverview(alien, {
    idOrName: "prj_mcytp6z3j91f7tn5ryqsfwtr",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("projectsGetCapabilityOverview failed:", res.error);
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
  useProjectsGetCapabilityOverview,
  useProjectsGetCapabilityOverviewSuspense,

  // Utility for prefetching data during server-side rendering and in React
  // Server Components that will be immediately available to client components
  // using the hooks.
  prefetchProjectsGetCapabilityOverview,

  // Utilities to invalidate the query cache for this query in response to
  // mutations and other user actions.
  invalidateProjectsGetCapabilityOverview,
  invalidateAllProjectsGetCapabilityOverview,
} from "@alienplatform/platform-api/react-query/projectsGetCapabilityOverview.js";
```
### Example Usage: projectName

<!-- UsageSnippet language="typescript" operationID="getProjectCapabilityOverview" method="get" path="/v1/projects/{idOrName}/project-capabilities/overview" example="projectName" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.projects.getCapabilityOverview({
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
import { projectsGetCapabilityOverview } from "@alienplatform/platform-api/funcs/projectsGetCapabilityOverview.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsGetCapabilityOverview(alien, {
    idOrName: "my-project",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("projectsGetCapabilityOverview failed:", res.error);
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
  useProjectsGetCapabilityOverview,
  useProjectsGetCapabilityOverviewSuspense,

  // Utility for prefetching data during server-side rendering and in React
  // Server Components that will be immediately available to client components
  // using the hooks.
  prefetchProjectsGetCapabilityOverview,

  // Utilities to invalidate the query cache for this query in response to
  // mutations and other user actions.
  invalidateProjectsGetCapabilityOverview,
  invalidateAllProjectsGetCapabilityOverview,
} from "@alienplatform/platform-api/react-query/projectsGetCapabilityOverview.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.GetProjectCapabilityOverviewRequest](../../models/operations/getprojectcapabilityoverviewrequest.md)                                                               | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.ProjectCapabilityOverview](../../models/projectcapabilityoverview.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 403, 404                 | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## getRemoteOperatorSummary

Get the authoritative Remote Operator project summary

### Example Usage: projectId

<!-- UsageSnippet language="typescript" operationID="getRemoteOperatorProjectSummary" method="get" path="/v1/projects/{idOrName}/remote-operator-summary" example="projectId" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.projects.getRemoteOperatorSummary({
    idOrName: "prj_mcytp6z3j91f7tn5ryqsfwtr",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { projectsGetRemoteOperatorSummary } from "@alienplatform/platform-api/funcs/projectsGetRemoteOperatorSummary.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsGetRemoteOperatorSummary(alien, {
    idOrName: "prj_mcytp6z3j91f7tn5ryqsfwtr",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("projectsGetRemoteOperatorSummary failed:", res.error);
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
  useProjectsGetRemoteOperatorSummary,
  useProjectsGetRemoteOperatorSummarySuspense,

  // Utility for prefetching data during server-side rendering and in React
  // Server Components that will be immediately available to client components
  // using the hooks.
  prefetchProjectsGetRemoteOperatorSummary,

  // Utilities to invalidate the query cache for this query in response to
  // mutations and other user actions.
  invalidateProjectsGetRemoteOperatorSummary,
  invalidateAllProjectsGetRemoteOperatorSummary,
} from "@alienplatform/platform-api/react-query/projectsGetRemoteOperatorSummary.js";
```
### Example Usage: projectName

<!-- UsageSnippet language="typescript" operationID="getRemoteOperatorProjectSummary" method="get" path="/v1/projects/{idOrName}/remote-operator-summary" example="projectName" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.projects.getRemoteOperatorSummary({
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
import { projectsGetRemoteOperatorSummary } from "@alienplatform/platform-api/funcs/projectsGetRemoteOperatorSummary.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsGetRemoteOperatorSummary(alien, {
    idOrName: "my-project",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("projectsGetRemoteOperatorSummary failed:", res.error);
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
  useProjectsGetRemoteOperatorSummary,
  useProjectsGetRemoteOperatorSummarySuspense,

  // Utility for prefetching data during server-side rendering and in React
  // Server Components that will be immediately available to client components
  // using the hooks.
  prefetchProjectsGetRemoteOperatorSummary,

  // Utilities to invalidate the query cache for this query in response to
  // mutations and other user actions.
  invalidateProjectsGetRemoteOperatorSummary,
  invalidateAllProjectsGetRemoteOperatorSummary,
} from "@alienplatform/platform-api/react-query/projectsGetRemoteOperatorSummary.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.GetRemoteOperatorProjectSummaryRequest](../../models/operations/getremoteoperatorprojectsummaryrequest.md)                                                         | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[operations.GetRemoteOperatorProjectSummaryResponse](../../models/operations/getremoteoperatorprojectsummaryresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 403, 404                 | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## acceptRemoteOperatorImage

Accept a reported image for a Remote Operator installation

### Example Usage: projectId

<!-- UsageSnippet language="typescript" operationID="acceptRemoteOperatorImage" method="post" path="/v1/projects/{idOrName}/remote-operator-summary/installations/{deploymentId}/accept-image" example="projectId" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.projects.acceptRemoteOperatorImage({
    idOrName: "prj_mcytp6z3j91f7tn5ryqsfwtr",
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
import { projectsAcceptRemoteOperatorImage } from "@alienplatform/platform-api/funcs/projectsAcceptRemoteOperatorImage.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsAcceptRemoteOperatorImage(alien, {
    idOrName: "prj_mcytp6z3j91f7tn5ryqsfwtr",
    deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("projectsAcceptRemoteOperatorImage failed:", res.error);
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
  useProjectsAcceptRemoteOperatorImageMutation
} from "@alienplatform/platform-api/react-query/projectsAcceptRemoteOperatorImage.js";
```
### Example Usage: projectName

<!-- UsageSnippet language="typescript" operationID="acceptRemoteOperatorImage" method="post" path="/v1/projects/{idOrName}/remote-operator-summary/installations/{deploymentId}/accept-image" example="projectName" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.projects.acceptRemoteOperatorImage({
    idOrName: "my-project",
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
import { projectsAcceptRemoteOperatorImage } from "@alienplatform/platform-api/funcs/projectsAcceptRemoteOperatorImage.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsAcceptRemoteOperatorImage(alien, {
    idOrName: "my-project",
    deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("projectsAcceptRemoteOperatorImage failed:", res.error);
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
  useProjectsAcceptRemoteOperatorImageMutation
} from "@alienplatform/platform-api/react-query/projectsAcceptRemoteOperatorImage.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.AcceptRemoteOperatorImageRequest](../../models/operations/acceptremoteoperatorimagerequest.md)                                                                     | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[operations.AcceptRemoteOperatorImageResponse](../../models/operations/acceptremoteoperatorimageresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 400, 404, 409            | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## acceptRemoteOperatorPermissions

Records the permissions compiled from every built-in operation plugin as the installation's installed permissions. Call it after re-applying the installation's Helm release, manifest, cloud access, or CloudFormation stack.

### Example Usage: projectId

<!-- UsageSnippet language="typescript" operationID="acceptRemoteOperatorPermissions" method="post" path="/v1/projects/{idOrName}/remote-operator-summary/installations/{deploymentId}/accept-permissions" example="projectId" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.projects.acceptRemoteOperatorPermissions({
    idOrName: "prj_mcytp6z3j91f7tn5ryqsfwtr",
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
import { projectsAcceptRemoteOperatorPermissions } from "@alienplatform/platform-api/funcs/projectsAcceptRemoteOperatorPermissions.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsAcceptRemoteOperatorPermissions(alien, {
    idOrName: "prj_mcytp6z3j91f7tn5ryqsfwtr",
    deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("projectsAcceptRemoteOperatorPermissions failed:", res.error);
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
  useProjectsAcceptRemoteOperatorPermissionsMutation
} from "@alienplatform/platform-api/react-query/projectsAcceptRemoteOperatorPermissions.js";
```
### Example Usage: projectName

<!-- UsageSnippet language="typescript" operationID="acceptRemoteOperatorPermissions" method="post" path="/v1/projects/{idOrName}/remote-operator-summary/installations/{deploymentId}/accept-permissions" example="projectName" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.projects.acceptRemoteOperatorPermissions({
    idOrName: "my-project",
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
import { projectsAcceptRemoteOperatorPermissions } from "@alienplatform/platform-api/funcs/projectsAcceptRemoteOperatorPermissions.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsAcceptRemoteOperatorPermissions(alien, {
    idOrName: "my-project",
    deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("projectsAcceptRemoteOperatorPermissions failed:", res.error);
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
  useProjectsAcceptRemoteOperatorPermissionsMutation
} from "@alienplatform/platform-api/react-query/projectsAcceptRemoteOperatorPermissions.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.AcceptRemoteOperatorPermissionsRequest](../../models/operations/acceptremoteoperatorpermissionsrequest.md)                                                         | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[operations.AcceptRemoteOperatorPermissionsResponse](../../models/operations/acceptremoteoperatorpermissionsresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 400, 403, 404, 409       | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## getAiUsage

### Example Usage: projectId

<!-- UsageSnippet language="typescript" operationID="getProjectAiUsage" method="get" path="/v1/projects/{idOrName}/ai-metrics" example="projectId" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.projects.getAiUsage({
    idOrName: "prj_mcytp6z3j91f7tn5ryqsfwtr",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { projectsGetAiUsage } from "@alienplatform/platform-api/funcs/projectsGetAiUsage.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsGetAiUsage(alien, {
    idOrName: "prj_mcytp6z3j91f7tn5ryqsfwtr",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("projectsGetAiUsage failed:", res.error);
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
  useProjectsGetAiUsage,
  useProjectsGetAiUsageSuspense,

  // Utility for prefetching data during server-side rendering and in React
  // Server Components that will be immediately available to client components
  // using the hooks.
  prefetchProjectsGetAiUsage,

  // Utilities to invalidate the query cache for this query in response to
  // mutations and other user actions.
  invalidateProjectsGetAiUsage,
  invalidateAllProjectsGetAiUsage,
} from "@alienplatform/platform-api/react-query/projectsGetAiUsage.js";
```
### Example Usage: projectName

<!-- UsageSnippet language="typescript" operationID="getProjectAiUsage" method="get" path="/v1/projects/{idOrName}/ai-metrics" example="projectName" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.projects.getAiUsage({
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
import { projectsGetAiUsage } from "@alienplatform/platform-api/funcs/projectsGetAiUsage.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsGetAiUsage(alien, {
    idOrName: "my-project",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("projectsGetAiUsage failed:", res.error);
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
  useProjectsGetAiUsage,
  useProjectsGetAiUsageSuspense,

  // Utility for prefetching data during server-side rendering and in React
  // Server Components that will be immediately available to client components
  // using the hooks.
  prefetchProjectsGetAiUsage,

  // Utilities to invalidate the query cache for this query in response to
  // mutations and other user actions.
  invalidateProjectsGetAiUsage,
  invalidateAllProjectsGetAiUsage,
} from "@alienplatform/platform-api/react-query/projectsGetAiUsage.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.GetProjectAiUsageRequest](../../models/operations/getprojectaiusagerequest.md)                                                                                     | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[operations.GetProjectAiUsageResponse](../../models/operations/getprojectaiusageresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 403, 404                 | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## getEncryptionUsage

### Example Usage: projectId

<!-- UsageSnippet language="typescript" operationID="getProjectEncryptionUsage" method="get" path="/v1/projects/{idOrName}/encryption-metrics" example="projectId" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.projects.getEncryptionUsage({
    idOrName: "prj_mcytp6z3j91f7tn5ryqsfwtr",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { projectsGetEncryptionUsage } from "@alienplatform/platform-api/funcs/projectsGetEncryptionUsage.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsGetEncryptionUsage(alien, {
    idOrName: "prj_mcytp6z3j91f7tn5ryqsfwtr",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("projectsGetEncryptionUsage failed:", res.error);
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
  useProjectsGetEncryptionUsage,
  useProjectsGetEncryptionUsageSuspense,

  // Utility for prefetching data during server-side rendering and in React
  // Server Components that will be immediately available to client components
  // using the hooks.
  prefetchProjectsGetEncryptionUsage,

  // Utilities to invalidate the query cache for this query in response to
  // mutations and other user actions.
  invalidateProjectsGetEncryptionUsage,
  invalidateAllProjectsGetEncryptionUsage,
} from "@alienplatform/platform-api/react-query/projectsGetEncryptionUsage.js";
```
### Example Usage: projectName

<!-- UsageSnippet language="typescript" operationID="getProjectEncryptionUsage" method="get" path="/v1/projects/{idOrName}/encryption-metrics" example="projectName" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.projects.getEncryptionUsage({
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
import { projectsGetEncryptionUsage } from "@alienplatform/platform-api/funcs/projectsGetEncryptionUsage.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsGetEncryptionUsage(alien, {
    idOrName: "my-project",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("projectsGetEncryptionUsage failed:", res.error);
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
  useProjectsGetEncryptionUsage,
  useProjectsGetEncryptionUsageSuspense,

  // Utility for prefetching data during server-side rendering and in React
  // Server Components that will be immediately available to client components
  // using the hooks.
  prefetchProjectsGetEncryptionUsage,

  // Utilities to invalidate the query cache for this query in response to
  // mutations and other user actions.
  invalidateProjectsGetEncryptionUsage,
  invalidateAllProjectsGetEncryptionUsage,
} from "@alienplatform/platform-api/react-query/projectsGetEncryptionUsage.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.GetProjectEncryptionUsageRequest](../../models/operations/getprojectencryptionusagerequest.md)                                                                     | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[operations.GetProjectEncryptionUsageResponse](../../models/operations/getprojectencryptionusageresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 403, 404                 | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## getSandboxMetrics

### Example Usage: projectId

<!-- UsageSnippet language="typescript" operationID="getProjectSandboxMetrics" method="get" path="/v1/projects/{idOrName}/sandbox-metrics" example="projectId" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.projects.getSandboxMetrics({
    idOrName: "prj_mcytp6z3j91f7tn5ryqsfwtr",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { projectsGetSandboxMetrics } from "@alienplatform/platform-api/funcs/projectsGetSandboxMetrics.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsGetSandboxMetrics(alien, {
    idOrName: "prj_mcytp6z3j91f7tn5ryqsfwtr",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("projectsGetSandboxMetrics failed:", res.error);
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
  useProjectsGetSandboxMetrics,
  useProjectsGetSandboxMetricsSuspense,

  // Utility for prefetching data during server-side rendering and in React
  // Server Components that will be immediately available to client components
  // using the hooks.
  prefetchProjectsGetSandboxMetrics,

  // Utilities to invalidate the query cache for this query in response to
  // mutations and other user actions.
  invalidateProjectsGetSandboxMetrics,
  invalidateAllProjectsGetSandboxMetrics,
} from "@alienplatform/platform-api/react-query/projectsGetSandboxMetrics.js";
```
### Example Usage: projectName

<!-- UsageSnippet language="typescript" operationID="getProjectSandboxMetrics" method="get" path="/v1/projects/{idOrName}/sandbox-metrics" example="projectName" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.projects.getSandboxMetrics({
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
import { projectsGetSandboxMetrics } from "@alienplatform/platform-api/funcs/projectsGetSandboxMetrics.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await projectsGetSandboxMetrics(alien, {
    idOrName: "my-project",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("projectsGetSandboxMetrics failed:", res.error);
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
  useProjectsGetSandboxMetrics,
  useProjectsGetSandboxMetricsSuspense,

  // Utility for prefetching data during server-side rendering and in React
  // Server Components that will be immediately available to client components
  // using the hooks.
  prefetchProjectsGetSandboxMetrics,

  // Utilities to invalidate the query cache for this query in response to
  // mutations and other user actions.
  invalidateProjectsGetSandboxMetrics,
  invalidateAllProjectsGetSandboxMetrics,
} from "@alienplatform/platform-api/react-query/projectsGetSandboxMetrics.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.GetProjectSandboxMetricsRequest](../../models/operations/getprojectsandboxmetricsrequest.md)                                                                       | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[operations.GetProjectSandboxMetricsResponse](../../models/operations/getprojectsandboxmetricsresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 403, 404                 | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |
