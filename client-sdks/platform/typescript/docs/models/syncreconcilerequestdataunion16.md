# SyncReconcileRequestDataUnion16


## Supported Types

### `models.DataAwsBedrock`

```typescript
const value: models.DataAwsBedrock = {
  availability: {
    catalogRevision: "<value>",
    models: [],
    source: "aws-bedrock",
  },
  region: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "timed-out",
        severity: "warning",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "deleting",
    partial: false,
    stale: false,
  },
  backend: "awsBedrock",
};
```

### `models.DataGcpVertex`

```typescript
const value: models.DataGcpVertex = {
  availability: {
    catalogRevision: "<value>",
    models: [
      {
        accessTest: "failed",
        availability: "blocked",
        blockers: [],
        clientApis: [
          "open-ai-chat-completions",
        ],
        publicModelId: "<id>",
      },
    ],
    source: "anthropic",
  },
  location: "<value>",
  project: "<value>",
  status: {
    collectionIssues: [],
    health: "degraded",
    lifecycle: "unknown",
    partial: false,
    stale: false,
  },
  backend: "gcpVertex",
};
```

### `models.DataAzureFoundry`

```typescript
const value: models.DataAzureFoundry = {
  accountName: "<value>",
  availability: {
    catalogRevision: "<value>",
    models: [
      {
        accessTest: "verified",
        availability: "available",
        blockers: [
          "quota-configuration-required",
        ],
        clientApis: [
          "open-ai-responses",
        ],
        publicModelId: "<id>",
      },
    ],
    source: "aws-bedrock",
  },
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "stopped",
    partial: false,
    stale: false,
  },
  backend: "azureFoundry",
};
```

### `models.DataExternal`

```typescript
const value: models.DataExternal = {
  provider: "<value>",
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "running",
    partial: true,
    stale: false,
  },
  backend: "external",
};
```

