# DataGcpServiceAccount1

## Example Usage

```typescript
import { DataGcpServiceAccount1 } from "@alienplatform/platform-api/models";

let value: DataGcpServiceAccount1 = {
  email: "Narciso53@hotmail.com",
  projectBindingCount: 780560,
  projectRoles: [
    "<value 1>",
    "<value 2>",
  ],
  serviceAccountBindingCount: 884958,
  serviceAccountRoles: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "collection-failed",
        severity: "error",
        source: "<value>",
      },
    ],
    health: "healthy",
    lifecycle: "stopped",
    partial: false,
    stale: true,
  },
  backend: "gcpServiceAccount",
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `description`                                                              | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `disabled`                                                                 | *boolean*                                                                  | :heavy_minus_sign:                                                         | N/A                                                                        |
| `displayName`                                                              | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `email`                                                                    | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `etag`                                                                     | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `name`                                                                     | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `oauth2ClientId`                                                           | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `projectBindingCount`                                                      | *number*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `projectId`                                                                | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `projectRoles`                                                             | *string*[]                                                                 | :heavy_check_mark:                                                         | N/A                                                                        |
| `serviceAccountBindingCount`                                               | *number*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `serviceAccountRoles`                                                      | *string*[]                                                                 | :heavy_check_mark:                                                         | N/A                                                                        |
| `status`                                                                   | [models.ResourceHeartbeatStatus41](../models/resourceheartbeatstatus41.md) | :heavy_check_mark:                                                         | N/A                                                                        |
| `uniqueId`                                                                 | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `backend`                                                                  | *"gcpServiceAccount"*                                                      | :heavy_check_mark:                                                         | N/A                                                                        |