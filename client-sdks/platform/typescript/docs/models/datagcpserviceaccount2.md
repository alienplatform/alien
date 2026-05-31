# DataGcpServiceAccount2

## Example Usage

```typescript
import { DataGcpServiceAccount2 } from "@alienplatform/platform-api/models";

let value: DataGcpServiceAccount2 = {
  impersonationGranted: false,
  roleBound: false,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "timed-out",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "healthy",
    lifecycle: "deleted",
    partial: false,
    stale: false,
  },
  backend: "gcpServiceAccount",
};
```

## Fields

| Field                                                      | Type                                                       | Required                                                   | Description                                                |
| ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- |
| `impersonationGranted`                                     | *boolean*                                                  | :heavy_check_mark:                                         | N/A                                                        |
| `roleBound`                                                | *boolean*                                                  | :heavy_check_mark:                                         | N/A                                                        |
| `serviceAccountEmail`                                      | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `serviceAccountUniqueId`                                   | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `status`                                                   | [models.HeartbeatStatus44](../models/heartbeatstatus44.md) | :heavy_check_mark:                                         | N/A                                                        |
| `backend`                                                  | *"gcpServiceAccount"*                                      | :heavy_check_mark:                                         | N/A                                                        |