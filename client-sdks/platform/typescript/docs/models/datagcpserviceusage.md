# DataGcpServiceUsage

## Example Usage

```typescript
import { DataGcpServiceUsage } from "@alienplatform/platform-api/models";

let value: DataGcpServiceUsage = {
  enabled: true,
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2025-06-11T19:58:11.111Z"),
      severity: "error",
    },
  ],
  projectId: "<id>",
  serviceName: "<value>",
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
    lifecycle: "creating",
    partial: true,
    stale: false,
  },
  backend: "gcpServiceUsage",
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `enabled`                                                                        | *boolean*                                                                        | :heavy_check_mark:                                                               | N/A                                                                              |
| `events`                                                                         | [models.SyncReconcileRequestEvent54](../models/syncreconcilerequestevent54.md)[] | :heavy_check_mark:                                                               | N/A                                                                              |
| `lastOperationName`                                                              | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `projectId`                                                                      | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `serviceName`                                                                    | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `serviceResourceName`                                                            | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `state`                                                                          | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `status`                                                                         | [models.HeartbeatStatus54](../models/heartbeatstatus54.md)                       | :heavy_check_mark:                                                               | N/A                                                                              |
| `title`                                                                          | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `backend`                                                                        | *"gcpServiceUsage"*                                                              | :heavy_check_mark:                                                               | N/A                                                                              |