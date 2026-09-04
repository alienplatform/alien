# SandboxHeartbeatData

Content-free telemetry about a sandbox's sessions.

Never anything from inside a session. A controller reaches only the cloud's management APIs,
and the whole point of the resource is that the control plane cannot see what runs in it.


## Supported Types

### `models.SandboxHeartbeatDataAwsMicrovm`

```typescript
const value: models.SandboxHeartbeatDataAwsMicrovm = {
  imageIdentifier: "<value>",
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "unknown",
    partial: true,
    stale: false,
  },
  backend: "awsMicrovm",
};
```

### `models.SandboxHeartbeatDataAzureSandboxGroup`

```typescript
const value: models.SandboxHeartbeatDataAzureSandboxGroup = {
  sandboxGroup: "<value>",
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "unknown",
    partial: true,
    stale: false,
  },
  backend: "azureSandboxGroup",
};
```

### `models.SandboxHeartbeatDataGcpAgentPlatform`

```typescript
const value: models.SandboxHeartbeatDataGcpAgentPlatform = {
  engine: "<value>",
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "unknown",
    partial: true,
    stale: false,
  },
  templateId: "<id>",
  backend: "gcpAgentPlatform",
};
```

### `models.SandboxHeartbeatDataKubernetesPods`

```typescript
const value: models.SandboxHeartbeatDataKubernetesPods = {
  activeSessions: 169819,
  idlePods: 607989,
  namespace: "<value>",
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "unknown",
    partial: true,
    stale: false,
  },
  backend: "kubernetesPods",
};
```

### `models.SandboxHeartbeatDataLocal`

```typescript
const value: models.SandboxHeartbeatDataLocal = {
  activeSessions: 159901,
  routeServing: false,
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "unknown",
    partial: true,
    stale: false,
  },
  backend: "local",
};
```
