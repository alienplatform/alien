# DataGcpServiceUsage

## Example Usage

```typescript
import { DataGcpServiceUsage } from "@alienplatform/platform-api/models/operations";

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
        severity: "info",
        source: "<value>",
      },
    ],
    health: "healthy",
    lifecycle: "stopping",
    partial: false,
    stale: true,
  },
  backend: "gcpServiceUsage",
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `enabled`                                                                                                | *boolean*                                                                                                | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `events`                                                                                                 | [operations.GetRawResourceHeartbeatEvent54](../../models/operations/getrawresourceheartbeatevent54.md)[] | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `lastOperationName`                                                                                      | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `projectId`                                                                                              | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `serviceName`                                                                                            | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `serviceResourceName`                                                                                    | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `state`                                                                                                  | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `status`                                                                                                 | [operations.DataStatus54](../../models/operations/datastatus54.md)                                       | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `title`                                                                                                  | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `backend`                                                                                                | *"gcpServiceUsage"*                                                                                      | :heavy_check_mark:                                                                                       | N/A                                                                                                      |