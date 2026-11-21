# DataAwsBedrock

## Example Usage

```typescript
import { DataAwsBedrock } from "@alienplatform/platform-api/models";

let value: DataAwsBedrock = {
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
        reason: "api-unavailable",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "running",
    partial: false,
    stale: false,
  },
  backend: "awsBedrock",
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `availability`                                                                             | [models.SyncReconcileRequestAvailability1](../models/syncreconcilerequestavailability1.md) | :heavy_check_mark:                                                                         | N/A                                                                                        |
| `region`                                                                                   | *string*                                                                                   | :heavy_check_mark:                                                                         | N/A                                                                                        |
| `status`                                                                                   | [models.ResourceHeartbeatStatus70](../models/resourceheartbeatstatus70.md)                 | :heavy_check_mark:                                                                         | N/A                                                                                        |
| `backend`                                                                                  | *"awsBedrock"*                                                                             | :heavy_check_mark:                                                                         | N/A                                                                                        |