# AiHeartbeatData


## Supported Types

### `models.AiHeartbeatDataAwsBedrock`

```typescript
const value: models.AiHeartbeatDataAwsBedrock = {
  region: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "creating",
    partial: false,
    stale: true,
  },
  backend: "awsBedrock",
};
```

### `models.AiHeartbeatDataGcpVertex`

```typescript
const value: models.AiHeartbeatDataGcpVertex = {
  location: "<value>",
  project: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "creating",
    partial: false,
    stale: true,
  },
  backend: "gcpVertex",
};
```

### `models.AiHeartbeatDataAzureFoundry`

```typescript
const value: models.AiHeartbeatDataAzureFoundry = {
  accountName: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "creating",
    partial: false,
    stale: true,
  },
  backend: "azureFoundry",
};
```

### `models.AiHeartbeatDataExternal`

```typescript
const value: models.AiHeartbeatDataExternal = {
  provider: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "creating",
    partial: false,
    stale: true,
  },
  backend: "external",
};
```
