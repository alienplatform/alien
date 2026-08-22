# SyncReconcileRequestDataUnion18

Content-free telemetry about a sandbox's sessions.

Never anything from inside a session. A controller reaches only the cloud's management APIs,
and the whole point of the resource is that the control plane cannot see what runs in it.


## Supported Types

### `models.DataAwsMicrovm`

```typescript
const value: models.DataAwsMicrovm = {
  imageIdentifier: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "collection-failed",
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

### `models.DataAzureSandboxGroup`

```typescript
const value: models.DataAzureSandboxGroup = {
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

### `models.DataKubernetesPods`

```typescript
const value: models.DataKubernetesPods = {
  activeSessions: 284392,
  idlePods: 443425,
  namespace: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "collection-failed",
        severity: "error",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "unknown",
    partial: false,
    stale: true,
  },
  backend: "kubernetesPods",
};
```

### `models.DataLocal12`

```typescript
const value: models.DataLocal12 = {
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

