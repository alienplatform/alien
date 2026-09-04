# DataUnion18

Content-free telemetry about a sandbox's sessions.

Never anything from inside a session. A controller reaches only the cloud's management APIs,
and the whole point of the resource is that the control plane cannot see what runs in it.


## Supported Types

### `operations.DataAwsMicrovm`

```typescript
const value: operations.DataAwsMicrovm = {
  imageIdentifier: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "timed-out",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "healthy",
    lifecycle: "updating",
    partial: true,
    stale: true,
  },
  backend: "awsMicrovm",
};
```

### `operations.DataAzureSandboxGroup`

```typescript
const value: operations.DataAzureSandboxGroup = {
  sandboxGroup: "<value>",
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "stopped",
    partial: true,
    stale: false,
  },
  backend: "azureSandboxGroup",
};
```

### `operations.DataGcpAgentPlatform`

```typescript
const value: operations.DataGcpAgentPlatform = {
  engine: "<value>",
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "stopping",
    partial: false,
    stale: false,
  },
  templateId: "<id>",
  backend: "gcpAgentPlatform",
};
```

### `operations.DataKubernetesPods`

```typescript
const value: operations.DataKubernetesPods = {
  activeSessions: 284392,
  idlePods: 443425,
  namespace: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "warning",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "deleted",
    partial: false,
    stale: true,
  },
  backend: "kubernetesPods",
};
```

### `operations.DataLocal12`

```typescript
const value: operations.DataLocal12 = {
  activeSessions: 111146,
  routeServing: true,
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "deleted",
    partial: true,
    stale: true,
  },
  backend: "local",
};
```

