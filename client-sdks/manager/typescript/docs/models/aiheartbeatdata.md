# AiHeartbeatData


## Supported Types

### `models.AiHeartbeatDataAwsBedrock`

```typescript
const value: models.AiHeartbeatDataAwsBedrock = {
  availability: {
    catalogRevision: "<value>",
    models: [
      {
        accessTest: "not-checked",
        availability: "available",
        blockers: [
          "agreement-required",
        ],
        clientApis: [
          "anthropic-messages",
        ],
        publicModelId: "<id>",
      },
    ],
    source: "aws-bedrock",
  },
  region: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "not-installed",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unknown",
    lifecycle: "running",
    partial: true,
    stale: true,
  },
  backend: "awsBedrock",
};
```

### `models.AiHeartbeatDataGcpVertex`

```typescript
const value: models.AiHeartbeatDataGcpVertex = {
  availability: {
    catalogRevision: "<value>",
    models: [
      {
        accessTest: "not-checked",
        availability: "available",
        blockers: [
          "agreement-required",
        ],
        clientApis: [
          "anthropic-messages",
        ],
        publicModelId: "<id>",
      },
    ],
    source: "aws-bedrock",
  },
  location: "<value>",
  project: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "not-installed",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unknown",
    lifecycle: "running",
    partial: true,
    stale: true,
  },
  backend: "gcpVertex",
};
```

### `models.AiHeartbeatDataAzureFoundry`

```typescript
const value: models.AiHeartbeatDataAzureFoundry = {
  accountName: "<value>",
  availability: {
    catalogRevision: "<value>",
    models: [
      {
        accessTest: "not-checked",
        availability: "available",
        blockers: [
          "agreement-required",
        ],
        clientApis: [
          "anthropic-messages",
        ],
        publicModelId: "<id>",
      },
    ],
    source: "aws-bedrock",
  },
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "not-installed",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unknown",
    lifecycle: "running",
    partial: true,
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
        reason: "not-installed",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unknown",
    lifecycle: "running",
    partial: true,
    stale: true,
  },
  backend: "external",
};
```
