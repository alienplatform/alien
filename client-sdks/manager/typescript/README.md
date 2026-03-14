# @alienplatform/manager-api

Developer-friendly & type-safe Typescript SDK specifically catered to leverage *@alienplatform/manager-api* API.

[![Built by Speakeasy](https://img.shields.io/badge/Built_by-SPEAKEASY-374151?style=for-the-badge&labelColor=f3f4f6)](https://www.speakeasy.com/?utm_source=@alienplatform/manager-api&utm_campaign=typescript)
[![License: MIT](https://img.shields.io/badge/LICENSE_//_MIT-3b5bdb?style=for-the-badge&labelColor=eff6ff)](https://opensource.org/licenses/MIT)


<br /><br />
> [!IMPORTANT]
> This SDK is not yet ready for production use. To complete setup please follow the steps outlined in your [workspace](https://app.speakeasy.com/org/alien/alien). Delete this section before > publishing to a package manager.

<!-- Start Summary [summary] -->
## Summary

Alien Server API: Control plane for Alien applications. Manages deployments, releases, commands, and telemetry.
<!-- End Summary [summary] -->

<!-- Start Table of Contents [toc] -->
## Table of Contents
<!-- $toc-max-depth=2 -->
* [@alienplatform/manager-api](#alienplatformmanager-api)
  * [SDK Installation](#sdk-installation)
  * [Requirements](#requirements)
  * [SDK Example Usage](#sdk-example-usage)
  * [Authentication](#authentication)
  * [Available Resources and Operations](#available-resources-and-operations)
  * [Standalone functions](#standalone-functions)
  * [Retries](#retries)
  * [Error Handling](#error-handling)
  * [Custom HTTP Client](#custom-http-client)
  * [Debugging](#debugging)
* [Development](#development)
  * [Maturity](#maturity)
  * [Contributions](#contributions)

<!-- End Table of Contents [toc] -->

<!-- Start SDK Installation [installation] -->
## SDK Installation

> [!TIP]
> To finish publishing your SDK to npm and others you must [run your first generation action](https://www.speakeasy.com/docs/github-setup#step-by-step-guide).


The SDK can be installed with either [npm](https://www.npmjs.com/), [pnpm](https://pnpm.io/), [bun](https://bun.sh/) or [yarn](https://classic.yarnpkg.com/en/) package managers.

### NPM

```bash
npm add <UNSET>
```

### PNPM

```bash
pnpm add <UNSET>
```

### Bun

```bash
bun add <UNSET>
```

### Yarn

```bash
yarn add <UNSET>
```

> [!NOTE]
> This package is published as an ES Module (ESM) only. For applications using
> CommonJS, use `await import()` to import and use this package.
<!-- End SDK Installation [installation] -->

<!-- Start Requirements [requirements] -->
## Requirements

For supported JavaScript runtimes, please consult [RUNTIMES.md](RUNTIMES.md).
<!-- End Requirements [requirements] -->

<!-- Start SDK Example Usage [usage] -->
## SDK Example Usage

### Example

```typescript
import { AlienManager } from "@alienplatform/manager-api";

const alienManager = new AlienManager({
  serverURL: "https://api.example.com",
  bearer: process.env["ALIEN_MANAGER_BEARER"] ?? "",
});

async function run() {
  const result = await alienManager.health.health();

  console.log(result);
}

run();

```
<!-- End SDK Example Usage [usage] -->

<!-- Start Authentication [security] -->
## Authentication

### Per-Client Security Schemes

This SDK supports the following security scheme globally:

| Name     | Type   | Scheme  | Environment Variable   |
| -------- | ------ | ------- | ---------------------- |
| `bearer` | apiKey | API key | `ALIEN_MANAGER_BEARER` |

To authenticate with the API the `bearer` parameter must be set when initializing the SDK client instance. For example:
```typescript
import { AlienManager } from "@alienplatform/manager-api";

const alienManager = new AlienManager({
  serverURL: "https://api.example.com",
  bearer: process.env["ALIEN_MANAGER_BEARER"] ?? "",
});

async function run() {
  const result = await alienManager.health.health();

  console.log(result);
}

run();

```
<!-- End Authentication [security] -->

<!-- Start Available Resources and Operations [operations] -->
## Available Resources and Operations

<details open>
<summary>Available methods</summary>

### [Credentials](docs/sdks/credentials/README.md)

* [resolveCredentials](docs/sdks/credentials/README.md#resolvecredentials)

### [DeploymentGroups](docs/sdks/deploymentgroups/README.md)

* [listDeploymentGroups](docs/sdks/deploymentgroups/README.md#listdeploymentgroups)
* [createDeploymentGroup](docs/sdks/deploymentgroups/README.md#createdeploymentgroup)
* [getDeploymentGroup](docs/sdks/deploymentgroups/README.md#getdeploymentgroup)
* [createDeploymentGroupToken](docs/sdks/deploymentgroups/README.md#createdeploymentgrouptoken)

### [Deployments](docs/sdks/deployments/README.md)

* [listDeployments](docs/sdks/deployments/README.md#listdeployments)
* [createDeployment](docs/sdks/deployments/README.md#createdeployment)
* [getDeployment](docs/sdks/deployments/README.md#getdeployment)
* [deleteDeployment](docs/sdks/deployments/README.md#deletedeployment)
* [getDeploymentInfo](docs/sdks/deployments/README.md#getdeploymentinfo)
* [redeploy](docs/sdks/deployments/README.md#redeploy)
* [retryDeployment](docs/sdks/deployments/README.md#retrydeployment)

### [Health](docs/sdks/health/README.md)

* [health](docs/sdks/health/README.md#health)

### [Releases](docs/sdks/releases/README.md)

* [createRelease](docs/sdks/releases/README.md#createrelease)
* [getLatestRelease](docs/sdks/releases/README.md#getlatestrelease)
* [getRelease](docs/sdks/releases/README.md#getrelease)

### [Sync](docs/sdks/sync/README.md)

* [initialize](docs/sdks/sync/README.md#initialize)
* [operatorSync](docs/sdks/sync/README.md#operatorsync)
* [acquire](docs/sdks/sync/README.md#acquire)
* [reconcile](docs/sdks/sync/README.md#reconcile)
* [release](docs/sdks/sync/README.md#release)

### [Whoami](docs/sdks/whoami/README.md)

* [whoami](docs/sdks/whoami/README.md#whoami)

</details>
<!-- End Available Resources and Operations [operations] -->

<!-- Start Standalone functions [standalone-funcs] -->
## Standalone functions

All the methods listed above are available as standalone functions. These
functions are ideal for use in applications running in the browser, serverless
runtimes or other environments where application bundle size is a primary
concern. When using a bundler to build your application, all unused
functionality will be either excluded from the final bundle or tree-shaken away.

To read more about standalone functions, check [FUNCTIONS.md](./FUNCTIONS.md).

<details>

<summary>Available standalone functions</summary>

- [`credentialsResolveCredentials`](docs/sdks/credentials/README.md#resolvecredentials)
- [`deploymentGroupsCreateDeploymentGroup`](docs/sdks/deploymentgroups/README.md#createdeploymentgroup)
- [`deploymentGroupsCreateDeploymentGroupToken`](docs/sdks/deploymentgroups/README.md#createdeploymentgrouptoken)
- [`deploymentGroupsGetDeploymentGroup`](docs/sdks/deploymentgroups/README.md#getdeploymentgroup)
- [`deploymentGroupsListDeploymentGroups`](docs/sdks/deploymentgroups/README.md#listdeploymentgroups)
- [`deploymentsCreateDeployment`](docs/sdks/deployments/README.md#createdeployment)
- [`deploymentsDeleteDeployment`](docs/sdks/deployments/README.md#deletedeployment)
- [`deploymentsGetDeployment`](docs/sdks/deployments/README.md#getdeployment)
- [`deploymentsGetDeploymentInfo`](docs/sdks/deployments/README.md#getdeploymentinfo)
- [`deploymentsListDeployments`](docs/sdks/deployments/README.md#listdeployments)
- [`deploymentsRedeploy`](docs/sdks/deployments/README.md#redeploy)
- [`deploymentsRetryDeployment`](docs/sdks/deployments/README.md#retrydeployment)
- [`healthHealth`](docs/sdks/health/README.md#health)
- [`releasesCreateRelease`](docs/sdks/releases/README.md#createrelease)
- [`releasesGetLatestRelease`](docs/sdks/releases/README.md#getlatestrelease)
- [`releasesGetRelease`](docs/sdks/releases/README.md#getrelease)
- [`syncAcquire`](docs/sdks/sync/README.md#acquire)
- [`syncInitialize`](docs/sdks/sync/README.md#initialize)
- [`syncOperatorSync`](docs/sdks/sync/README.md#operatorsync)
- [`syncReconcile`](docs/sdks/sync/README.md#reconcile)
- [`syncRelease`](docs/sdks/sync/README.md#release)
- [`whoamiWhoami`](docs/sdks/whoami/README.md#whoami)

</details>
<!-- End Standalone functions [standalone-funcs] -->

<!-- Start Retries [retries] -->
## Retries

Some of the endpoints in this SDK support retries.  If you use the SDK without any configuration, it will fall back to the default retry strategy provided by the API.  However, the default retry strategy can be overridden on a per-operation basis, or across the entire SDK.

To change the default retry strategy for a single API call, simply provide a retryConfig object to the call:
```typescript
import { AlienManager } from "@alienplatform/manager-api";

const alienManager = new AlienManager({
  serverURL: "https://api.example.com",
  bearer: process.env["ALIEN_MANAGER_BEARER"] ?? "",
});

async function run() {
  const result = await alienManager.health.health({
    retries: {
      strategy: "backoff",
      backoff: {
        initialInterval: 1,
        maxInterval: 50,
        exponent: 1.1,
        maxElapsedTime: 100,
      },
      retryConnectionErrors: false,
    },
  });

  console.log(result);
}

run();

```

If you'd like to override the default retry strategy for all operations that support retries, you can provide a retryConfig at SDK initialization:
```typescript
import { AlienManager } from "@alienplatform/manager-api";

const alienManager = new AlienManager({
  serverURL: "https://api.example.com",
  retryConfig: {
    strategy: "backoff",
    backoff: {
      initialInterval: 1,
      maxInterval: 50,
      exponent: 1.1,
      maxElapsedTime: 100,
    },
    retryConnectionErrors: false,
  },
  bearer: process.env["ALIEN_MANAGER_BEARER"] ?? "",
});

async function run() {
  const result = await alienManager.health.health();

  console.log(result);
}

run();

```
<!-- End Retries [retries] -->

<!-- Start Error Handling [errors] -->
## Error Handling

[`AlienManagerError`](./src/models/errors/alienmanagererror.ts) is the base class for all HTTP error responses. It has the following properties:

| Property            | Type       | Description                                            |
| ------------------- | ---------- | ------------------------------------------------------ |
| `error.message`     | `string`   | Error message                                          |
| `error.statusCode`  | `number`   | HTTP response status code eg `404`                     |
| `error.headers`     | `Headers`  | HTTP response headers                                  |
| `error.body`        | `string`   | HTTP body. Can be empty string if no body is returned. |
| `error.rawResponse` | `Response` | Raw HTTP response                                      |

### Example
```typescript
import { AlienManager } from "@alienplatform/manager-api";
import * as errors from "@alienplatform/manager-api/models/errors";

const alienManager = new AlienManager({
  serverURL: "https://api.example.com",
  bearer: process.env["ALIEN_MANAGER_BEARER"] ?? "",
});

async function run() {
  try {
    const result = await alienManager.health.health();

    console.log(result);
  } catch (error) {
    if (error instanceof errors.AlienManagerError) {
      console.log(error.message);
      console.log(error.statusCode);
      console.log(error.body);
      console.log(error.headers);
    }
  }
}

run();

```

### Error Classes
**Primary error:**
* [`AlienManagerError`](./src/models/errors/alienmanagererror.ts): The base class for HTTP error responses.

<details><summary>Less common errors (6)</summary>

<br />

**Network errors:**
* [`ConnectionError`](./src/models/errors/httpclienterrors.ts): HTTP client was unable to make a request to a server.
* [`RequestTimeoutError`](./src/models/errors/httpclienterrors.ts): HTTP request timed out due to an AbortSignal signal.
* [`RequestAbortedError`](./src/models/errors/httpclienterrors.ts): HTTP request was aborted by the client.
* [`InvalidRequestError`](./src/models/errors/httpclienterrors.ts): Any input used to create a request is invalid.
* [`UnexpectedClientError`](./src/models/errors/httpclienterrors.ts): Unrecognised or unexpected error.


**Inherit from [`AlienManagerError`](./src/models/errors/alienmanagererror.ts)**:
* [`ResponseValidationError`](./src/models/errors/responsevalidationerror.ts): Type mismatch between the data returned from the server and the structure expected by the SDK. See `error.rawValue` for the raw value and `error.pretty()` for a nicely formatted multi-line string.

</details>
<!-- End Error Handling [errors] -->

<!-- Start Custom HTTP Client [http-client] -->
## Custom HTTP Client

The TypeScript SDK makes API calls using an `HTTPClient` that wraps the native
[Fetch API](https://developer.mozilla.org/en-US/docs/Web/API/Fetch_API). This
client is a thin wrapper around `fetch` and provides the ability to attach hooks
around the request lifecycle that can be used to modify the request or handle
errors and response.

The `HTTPClient` constructor takes an optional `fetcher` argument that can be
used to integrate a third-party HTTP client or when writing tests to mock out
the HTTP client and feed in fixtures.

The following example shows how to use the `"beforeRequest"` hook to to add a
custom header and a timeout to requests and how to use the `"requestError"` hook
to log errors:

```typescript
import { AlienManager } from "@alienplatform/manager-api";
import { HTTPClient } from "@alienplatform/manager-api/lib/http";

const httpClient = new HTTPClient({
  // fetcher takes a function that has the same signature as native `fetch`.
  fetcher: (request) => {
    return fetch(request);
  }
});

httpClient.addHook("beforeRequest", (request) => {
  const nextRequest = new Request(request, {
    signal: request.signal || AbortSignal.timeout(5000)
  });

  nextRequest.headers.set("x-custom-header", "custom value");

  return nextRequest;
});

httpClient.addHook("requestError", (error, request) => {
  console.group("Request Error");
  console.log("Reason:", `${error}`);
  console.log("Endpoint:", `${request.method} ${request.url}`);
  console.groupEnd();
});

const sdk = new AlienManager({ httpClient: httpClient });
```
<!-- End Custom HTTP Client [http-client] -->

<!-- Start Debugging [debug] -->
## Debugging

You can setup your SDK to emit debug logs for SDK requests and responses.

You can pass a logger that matches `console`'s interface as an SDK option.

> [!WARNING]
> Beware that debug logging will reveal secrets, like API tokens in headers, in log messages printed to a console or files. It's recommended to use this feature only during local development and not in production.

```typescript
import { AlienManager } from "@alienplatform/manager-api";

const sdk = new AlienManager({ debugLogger: console });
```

You can also enable a default debug logger by setting an environment variable `ALIEN_MANAGER_DEBUG` to true.
<!-- End Debugging [debug] -->

<!-- Placeholder for Future Speakeasy SDK Sections -->

# Development

## Maturity

This SDK is in beta, and there may be breaking changes between versions without a major version update. Therefore, we recommend pinning usage
to a specific package version. This way, you can install the same version each time without breaking changes unless you are intentionally
looking for the latest version.

## Contributions

While we value open-source contributions to this SDK, this library is generated programmatically. Any manual changes added to internal files will be overwritten on the next generation. 
We look forward to hearing your feedback. Feel free to open a PR or an issue with a proof of concept and we'll do our best to include it in a future release. 

### SDK Created by [Speakeasy](https://www.speakeasy.com/?utm_source=@alienplatform/manager-api&utm_campaign=typescript)
