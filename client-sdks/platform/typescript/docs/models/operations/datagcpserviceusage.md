# DataGcpServiceUsage

## Example Usage

```typescript
import { DataGcpServiceUsage } from "@alienplatform/platform-api/models/operations";

let value: DataGcpServiceUsage = {
  enabled: true,
  projectId: "<id>",
  serviceName: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "api-unavailable",
        severity: "error",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "stopped",
    partial: true,
    stale: true,
  },
  backend: "gcpServiceUsage",
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `enabled`                                                          | *boolean*                                                          | :heavy_check_mark:                                                 | N/A                                                                |
| `lastOperationName`                                                | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `projectId`                                                        | *string*                                                           | :heavy_check_mark:                                                 | N/A                                                                |
| `serviceName`                                                      | *string*                                                           | :heavy_check_mark:                                                 | N/A                                                                |
| `serviceResourceName`                                              | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `state`                                                            | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `status`                                                           | [operations.DataStatus60](../../models/operations/datastatus60.md) | :heavy_check_mark:                                                 | N/A                                                                |
| `title`                                                            | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `backend`                                                          | *"gcpServiceUsage"*                                                | :heavy_check_mark:                                                 | N/A                                                                |