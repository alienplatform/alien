# DataGcpServiceAccount2

## Example Usage

```typescript
import { DataGcpServiceAccount2 } from "@alienplatform/platform-api/models/operations";

let value: DataGcpServiceAccount2 = {
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2026-09-28T13:47:01.596Z"),
      severity: "error",
    },
  ],
  impersonationGranted: true,
  roleBound: false,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "api-unavailable",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unknown",
    lifecycle: "deleting",
    partial: false,
    stale: false,
  },
  backend: "gcpServiceAccount",
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `events`                                                                                                 | [operations.GetRawResourceHeartbeatEvent44](../../models/operations/getrawresourceheartbeatevent44.md)[] | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `impersonationGranted`                                                                                   | *boolean*                                                                                                | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `roleBound`                                                                                              | *boolean*                                                                                                | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `serviceAccountEmail`                                                                                    | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `serviceAccountUniqueId`                                                                                 | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `status`                                                                                                 | [operations.DataStatus44](../../models/operations/datastatus44.md)                                       | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `backend`                                                                                                | *"gcpServiceAccount"*                                                                                    | :heavy_check_mark:                                                                                       | N/A                                                                                                      |