# DataGcpServiceAccount1

## Example Usage

```typescript
import { DataGcpServiceAccount1 } from "@alienplatform/platform-api/models/operations";

let value: DataGcpServiceAccount1 = {
  email: "Narciso53@hotmail.com",
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2025-11-02T07:18:24.714Z"),
      severity: "error",
    },
  ],
  projectBindingCount: 974149,
  projectRoles: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
  serviceAccountBindingCount: 694359,
  serviceAccountRoles: [
    "<value 1>",
    "<value 2>",
  ],
  status: {
    collectionIssues: [],
    health: "degraded",
    lifecycle: "failed",
    partial: true,
    stale: true,
  },
  backend: "gcpServiceAccount",
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `description`                                                                                            | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `disabled`                                                                                               | *boolean*                                                                                                | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `displayName`                                                                                            | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `email`                                                                                                  | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `etag`                                                                                                   | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `events`                                                                                                 | [operations.GetRawResourceHeartbeatEvent37](../../models/operations/getrawresourceheartbeatevent37.md)[] | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `name`                                                                                                   | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `oauth2ClientId`                                                                                         | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `projectBindingCount`                                                                                    | *number*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `projectId`                                                                                              | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `projectRoles`                                                                                           | *string*[]                                                                                               | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `serviceAccountBindingCount`                                                                             | *number*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `serviceAccountRoles`                                                                                    | *string*[]                                                                                               | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `status`                                                                                                 | [operations.DataStatus37](../../models/operations/datastatus37.md)                                       | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `uniqueId`                                                                                               | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `backend`                                                                                                | *"gcpServiceAccount"*                                                                                    | :heavy_check_mark:                                                                                       | N/A                                                                                                      |