# SetupLinks

## Overview

### Available Operations

* [createPortalSession](#createportalsession) - Open a setup link
* [createPortalInstallerKey](#createportalinstallerkey) - Mint an installer key for a portal session
* [create](#create) - Create a customer setup link

## createPortalSession

Exchanges the code in a setup link URL for a deployment portal session. A link with allowed emails asks for an email, sends a code to it, and opens on the right code. A handoff link opens once, within 5 minutes of creation.

### Example Usage

<!-- UsageSnippet language="typescript" operationID="createPortalSession" method="post" path="/v1/portal/sessions" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien();

async function run() {
  const result = await alien.setupLinks.createPortalSession({
    link: "<value>",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { setupLinksCreatePortalSession } from "@alienplatform/platform-api/funcs/setupLinksCreatePortalSession.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore();

async function run() {
  const res = await setupLinksCreatePortalSession(alien, {
    link: "<value>",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("setupLinksCreatePortalSession failed:", res.error);
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
  useSetupLinksCreatePortalSessionMutation
} from "@alienplatform/platform-api/react-query/setupLinksCreatePortalSession.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [models.CreatePortalSessionRequest](../../models/createportalsessionrequest.md)                                                                                                | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.PortalSession](../../models/portalsession.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 401, 403, 410, 422, 429  | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## createPortalInstallerKey

Mints a 24-hour deployer API key for the setup link behind a portal session. The deployment portal shows it to the customer in install and teardown commands and gives it to the manager, so it is a separate key from the session: it stops working when the setup link is disabled, revoked or expires. Minting again never revokes an earlier key, since a command the customer copied may hold it; each key expires after 24 hours, and a session mints at most 10. Only a portal session token is accepted.

### Example Usage

<!-- UsageSnippet language="typescript" operationID="createPortalInstallerKey" method="post" path="/v1/portal/installer-key" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.setupLinks.createPortalInstallerKey();

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { setupLinksCreatePortalInstallerKey } from "@alienplatform/platform-api/funcs/setupLinksCreatePortalInstallerKey.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await setupLinksCreatePortalInstallerKey(alien);
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("setupLinksCreatePortalInstallerKey failed:", res.error);
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
  useSetupLinksCreatePortalInstallerKeyMutation
} from "@alienplatform/platform-api/react-query/setupLinksCreatePortalInstallerKey.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.PortalInstallerKey](../../models/portalinstallerkey.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 401, 403, 404, 409, 429  | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## create

Ensures the Deployment Group identified by Project and external ID, then creates a group-scoped setup link with exact captured sources.

### Example Usage

<!-- UsageSnippet language="typescript" operationID="createSetupLink" method="post" path="/v1/deployment-groups/setup-links" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.setupLinks.create({
    externalId: "ext_example_01",
    name: "prod-us-east-1",
    project: "<value>",
    deploymentSetupConfig: {
      validatedReleaseSelection: {
        releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
        releaseChannel: "<value>",
        platform: "kubernetes",
      },
    },
    recoveryDeploymentGroupId: "dg_r27ict8c7vcgsumpj90ackf7b",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienCore } from "@alienplatform/platform-api/core.js";
import { setupLinksCreate } from "@alienplatform/platform-api/funcs/setupLinksCreate.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  workspace: "my-workspace",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await setupLinksCreate(alien, {
    externalId: "ext_example_01",
    name: "prod-us-east-1",
    project: "<value>",
    deploymentSetupConfig: {
      validatedReleaseSelection: {
        releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
        releaseChannel: "<value>",
        platform: "kubernetes",
      },
    },
    recoveryDeploymentGroupId: "dg_r27ict8c7vcgsumpj90ackf7b",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("setupLinksCreate failed:", res.error);
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
  useSetupLinksCreateMutation
} from "@alienplatform/platform-api/react-query/setupLinksCreate.js";
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [models.CreateSetupLinkRequest](../../models/createsetuplinkrequest.md)                                                                                                        | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.CreateSetupLinkResponse](../../models/createsetuplinkresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 400, 404, 409, 422       | application/json         |
| errors.APIError          | 500                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |