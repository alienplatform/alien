# SyncReconcileRequestDataUnion19

Content-free telemetry about what a sandbox resource is running.

Never anything from inside a sandbox. A controller reaches only the cloud's management APIs,
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
        reason: "timed-out",
        severity: "error",
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

### `models.DataGcpAgentPlatform`

```typescript
const value: models.DataGcpAgentPlatform = {
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

### `models.DataLocal13`

```typescript
const value: models.DataLocal13 = {
  activeSessions: 879062,
  routeServing: false,
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
    lifecycle: "stopping",
    partial: true,
    stale: true,
  },
  backend: "local",
};
```
