# AgentSessions

## Overview

### Available Operations

* [list](#list) - List ai-agent monitor sessions for this workspace. Newest first, capped at 50.
* [get](#get) - Retrieve one ai-agent monitor session by id.
* [events](#events) - Incrementally read a session's event log (steps, tool calls, report deltas, approvals, status transitions). Pass the previous response's `latestSeq` as `after` to fetch only new events.
* [approve](#approve) - Approve a halted ai-agent monitor session. Proxies to the ai-agent service, minting a fresh CLI session for the caller so the ai-agent's own auth applies.
* [stop](#stop) - Stop (cancel) a running, queued, or halted ai-agent monitor session. Proxies to the ai-agent service, minting a fresh CLI session for the caller so the ai-agent's own auth applies. Idempotent — stopping an already-terminal session is a 200 no-op.

## list

List ai-agent monitor sessions for this workspace. Newest first, capped at 50.

### Example Usage

<!-- UsageSnippet language="typescript" operationID="listAgentSessions" method="get" path="/v1/agent-sessions" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.agentSessions.list({
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
import { agentSessionsList } from "@alienplatform/platform-api/funcs/agentSessionsList.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await agentSessionsList(alien, {
    workspace: "my-workspace",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("agentSessionsList failed:", res.error);
  }
}

run();
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.ListAgentSessionsRequest](../../models/operations/listagentsessionsrequest.md)                                                                                     | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.AgentSessionListResponse](../../models/agentsessionlistresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## get

Retrieve one ai-agent monitor session by id.

### Example Usage

<!-- UsageSnippet language="typescript" operationID="getAgentSession" method="get" path="/v1/agent-sessions/{id}" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.agentSessions.get({
    id: "<id>",
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
import { agentSessionsGet } from "@alienplatform/platform-api/funcs/agentSessionsGet.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await agentSessionsGet(alien, {
    id: "<id>",
    workspace: "my-workspace",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("agentSessionsGet failed:", res.error);
  }
}

run();
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.GetAgentSessionRequest](../../models/operations/getagentsessionrequest.md)                                                                                         | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.AgentSessionDetail](../../models/agentsessiondetail.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 404                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## events

Incrementally read a session's event log (steps, tool calls, report deltas, approvals, status transitions). Pass the previous response's `latestSeq` as `after` to fetch only new events.

### Example Usage

<!-- UsageSnippet language="typescript" operationID="listAgentSessionEvents" method="get" path="/v1/agent-sessions/{id}/events" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.agentSessions.events({
    id: "<id>",
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
import { agentSessionsEvents } from "@alienplatform/platform-api/funcs/agentSessionsEvents.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await agentSessionsEvents(alien, {
    id: "<id>",
    workspace: "my-workspace",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("agentSessionsEvents failed:", res.error);
  }
}

run();
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.ListAgentSessionEventsRequest](../../models/operations/listagentsessioneventsrequest.md)                                                                           | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.AgentSessionEventsResponse](../../models/agentsessioneventsresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 404                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## approve

Approve a halted ai-agent monitor session. Proxies to the ai-agent service, minting a fresh CLI session for the caller so the ai-agent's own auth applies.

### Example Usage

<!-- UsageSnippet language="typescript" operationID="approveAgentSession" method="post" path="/v1/agent-sessions/{id}/approve" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.agentSessions.approve({
    id: "<id>",
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
import { agentSessionsApprove } from "@alienplatform/platform-api/funcs/agentSessionsApprove.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await agentSessionsApprove(alien, {
    id: "<id>",
    workspace: "my-workspace",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("agentSessionsApprove failed:", res.error);
  }
}

run();
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.ApproveAgentSessionRequest](../../models/operations/approveagentsessionrequest.md)                                                                                 | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.AgentSessionApproveResponse](../../models/agentsessionapproveresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 404                      | application/json         |
| errors.APIError          | 503                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |

## stop

Stop (cancel) a running, queued, or halted ai-agent monitor session. Proxies to the ai-agent service, minting a fresh CLI session for the caller so the ai-agent's own auth applies. Idempotent — stopping an already-terminal session is a 200 no-op.

### Example Usage

<!-- UsageSnippet language="typescript" operationID="stopAgentSession" method="post" path="/v1/agent-sessions/{id}/stop" -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.agentSessions.stop({
    id: "<id>",
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
import { agentSessionsStop } from "@alienplatform/platform-api/funcs/agentSessionsStop.js";

// Use `AlienCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alien = new AlienCore({
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const res = await agentSessionsStop(alien, {
    id: "<id>",
    workspace: "my-workspace",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("agentSessionsStop failed:", res.error);
  }
}

run();
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.StopAgentSessionRequest](../../models/operations/stopagentsessionrequest.md)                                                                                       | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.AgentSessionStopResponse](../../models/agentsessionstopresponse.md)\>**

### Errors

| Error Type               | Status Code              | Content Type             |
| ------------------------ | ------------------------ | ------------------------ |
| errors.APIError          | 404                      | application/json         |
| errors.APIError          | 503                      | application/json         |
| errors.AlienDefaultError | 4XX, 5XX                 | \*/\*                    |
