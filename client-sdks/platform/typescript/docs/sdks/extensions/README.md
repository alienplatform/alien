# Extensions

## Overview

### Available Operations

* [createBundleUploadUrls](#createbundleuploadurls) - Name every object of an extension bundle and get a presigned PUT for each one the project does not have yet. PUT them, then call POST /extensions/bundles.
* [publishBundle](#publishbundle) - Check an uploaded extension bundle against its core zip's manifest and store each object under its sha256. A release that pins the core zip's digest can be created after.
* [getBundleDownloadUrl](#getbundledownloadurl) - A presigned URL to download one stored object of an extension bundle by its digest. Whoever reads the project's releases may download what they pin.

## createBundleUploadUrls

Name every object of an extension bundle and get a presigned PUT for each one the project does not have yet. PUT them, then call POST /extensions/bundles.

### Example Usage: projectId

<!-- UsageSnippet language="typescript" operationID="createExtensionBundleUploadUrls" method="post" path="/v1/extensions/bundles/upload-urls" example="projectId" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.extensions.createBundleUploadUrls("prj_mcytp6z3j91f7tn5ryqsfwtrdqkv");

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { extensionsCreateBundleUploadUrls } from "@alienplatform/platform-api/funcs/extensionsCreateBundleUploadUrls.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await extensionsCreateBundleUploadUrls(alien, "prj_mcytp6z3j91f7tn5ryqsfwtrdqkv");
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("extensionsCreateBundleUploadUrls failed:", res.error);
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
  useExtensionsCreateBundleUploadUrlsMutation
} from "@alienplatform/platform-api/react-query/extensionsCreateBundleUploadUrls.js";
```
### Example Usage: projectName

<!-- UsageSnippet language="typescript" operationID="createExtensionBundleUploadUrls" method="post" path="/v1/extensions/bundles/upload-urls" example="projectName" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.extensions.createBundleUploadUrls("my-project");

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { extensionsCreateBundleUploadUrls } from "@alienplatform/platform-api/funcs/extensionsCreateBundleUploadUrls.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await extensionsCreateBundleUploadUrls(alien, "my-project");
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("extensionsCreateBundleUploadUrls failed:", res.error);
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
  useExtensionsCreateBundleUploadUrlsMutation
} from "@alienplatform/platform-api/react-query/extensionsCreateBundleUploadUrls.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `project`                                                                                                                                                                      | *string*                                                                                                                                                                       | :heavy_check_mark:                                                                                                                                                             | Filter by project ID or name.                                                                                                                                                  |
| `extensionBundleUploadUrlsRequest`                                                                                                                                             | [models.ExtensionBundleUploadUrlsRequest](../../models/extensionbundleuploadurlsrequest.md)                                                                                    | :heavy_minus_sign:                                                                                                                                                             | N/A                                                                                                                                                                            |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.ExtensionBundleUploadUrlsResponse](../../models/extensionbundleuploadurlsresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 400, 403, 404, 422       | application/json         |
| errors.APIError          | 500, 502, 503            | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## publishBundle

Check an uploaded extension bundle against its core zip's manifest and store each object under its sha256. A release that pins the core zip's digest can be created after.

### Example Usage: projectId

<!-- UsageSnippet language="typescript" operationID="publishExtensionBundle" method="post" path="/v1/extensions/bundles" example="projectId" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.extensions.publishBundle("prj_mcytp6z3j91f7tn5ryqsfwtrdqkv");

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { extensionsPublishBundle } from "@alienplatform/platform-api/funcs/extensionsPublishBundle.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await extensionsPublishBundle(alien, "prj_mcytp6z3j91f7tn5ryqsfwtrdqkv");
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("extensionsPublishBundle failed:", res.error);
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
  useExtensionsPublishBundleMutation
} from "@alienplatform/platform-api/react-query/extensionsPublishBundle.js";
```
### Example Usage: projectName

<!-- UsageSnippet language="typescript" operationID="publishExtensionBundle" method="post" path="/v1/extensions/bundles" example="projectName" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.extensions.publishBundle("my-project");

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { extensionsPublishBundle } from "@alienplatform/platform-api/funcs/extensionsPublishBundle.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await extensionsPublishBundle(alien, "my-project");
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("extensionsPublishBundle failed:", res.error);
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
  useExtensionsPublishBundleMutation
} from "@alienplatform/platform-api/react-query/extensionsPublishBundle.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `project`                                                                                                                                                                      | *string*                                                                                                                                                                       | :heavy_check_mark:                                                                                                                                                             | Filter by project ID or name.                                                                                                                                                  |
| `publishExtensionBundleRequest`                                                                                                                                                | [models.PublishExtensionBundleRequest](../../models/publishextensionbundlerequest.md)                                                                                          | :heavy_minus_sign:                                                                                                                                                             | N/A                                                                                                                                                                            |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.PublishExtensionBundleResponse](../../models/publishextensionbundleresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 400, 403, 404, 422       | application/json         |
| errors.APIError          | 500, 502, 503            | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## getBundleDownloadUrl

A presigned URL to download one stored object of an extension bundle by its digest. Whoever reads the project's releases may download what they pin.

### Example Usage: projectId

<!-- UsageSnippet language="typescript" operationID="getExtensionBundleDownloadUrl" method="get" path="/v1/extensions/bundles/{digest}/download-url" example="projectId" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.extensions.getBundleDownloadUrl({
    digest: "<value>",
    project: "prj_mcytp6z3j91f7tn5ryqsfwtrdqkv",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { extensionsGetBundleDownloadUrl } from "@alienplatform/platform-api/funcs/extensionsGetBundleDownloadUrl.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await extensionsGetBundleDownloadUrl(alien, {
    digest: "<value>",
    project: "prj_mcytp6z3j91f7tn5ryqsfwtrdqkv",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("extensionsGetBundleDownloadUrl failed:", res.error);
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
  useExtensionsGetBundleDownloadUrl,
  useExtensionsGetBundleDownloadUrlSuspense,

  // Utility for prefetching data during server-side rendering and in React
  // Server Components that will be immediately available to client components
  // using the hooks.
  prefetchExtensionsGetBundleDownloadUrl,

  // Utilities to invalidate the query cache for this query in response to
  // mutations and other user actions.
  invalidateExtensionsGetBundleDownloadUrl,
  invalidateAllExtensionsGetBundleDownloadUrl,
} from "@alienplatform/platform-api/react-query/extensionsGetBundleDownloadUrl.js";
```
### Example Usage: projectName

<!-- UsageSnippet language="typescript" operationID="getExtensionBundleDownloadUrl" method="get" path="/v1/extensions/bundles/{digest}/download-url" example="projectName" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.extensions.getBundleDownloadUrl({
    digest: "<value>",
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
import { extensionsGetBundleDownloadUrl } from "@alienplatform/platform-api/funcs/extensionsGetBundleDownloadUrl.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await extensionsGetBundleDownloadUrl(alien, {
    digest: "<value>",
    project: "my-project",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("extensionsGetBundleDownloadUrl failed:", res.error);
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
  useExtensionsGetBundleDownloadUrl,
  useExtensionsGetBundleDownloadUrlSuspense,

  // Utility for prefetching data during server-side rendering and in React
  // Server Components that will be immediately available to client components
  // using the hooks.
  prefetchExtensionsGetBundleDownloadUrl,

  // Utilities to invalidate the query cache for this query in response to
  // mutations and other user actions.
  invalidateExtensionsGetBundleDownloadUrl,
  invalidateAllExtensionsGetBundleDownloadUrl,
} from "@alienplatform/platform-api/react-query/extensionsGetBundleDownloadUrl.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.GetExtensionBundleDownloadUrlRequest](../../models/operations/getextensionbundledownloadurlrequest.md)                                                             | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.ExtensionBundleDownloadUrlResponse](../../models/extensionbundledownloadurlresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 400, 403, 404, 422       | application/json         |
| errors.APIError          | 500, 502, 503            | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |