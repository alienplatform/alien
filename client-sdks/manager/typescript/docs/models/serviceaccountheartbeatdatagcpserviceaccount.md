# ServiceAccountHeartbeatDataGcpServiceAccount

## Example Usage

```typescript
import { ServiceAccountHeartbeatDataGcpServiceAccount } from "@alienplatform/manager-api/models";

let value: ServiceAccountHeartbeatDataGcpServiceAccount = {
  email: "Kraig_Jast-Koss80@yahoo.com",
  projectBindingCount: 864516,
  projectRoles: [
    "<value 1>",
    "<value 2>",
  ],
  serviceAccountBindingCount: 255611,
  serviceAccountRoles: [
    "<value 1>",
    "<value 2>",
  ],
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "stopped",
    partial: false,
    stale: true,
  },
  backend: "gcpServiceAccount",
};
```

## Fields

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `description`                                                                      | *string*                                                                           | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `disabled`                                                                         | *boolean*                                                                          | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `displayName`                                                                      | *string*                                                                           | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `email`                                                                            | *string*                                                                           | :heavy_check_mark:                                                                 | N/A                                                                                |
| `etag`                                                                             | *string*                                                                           | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `name`                                                                             | *string*                                                                           | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `oauth2ClientId`                                                                   | *string*                                                                           | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `projectBindingCount`                                                              | *number*                                                                           | :heavy_check_mark:                                                                 | N/A                                                                                |
| `projectId`                                                                        | *string*                                                                           | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `projectRoles`                                                                     | *string*[]                                                                         | :heavy_check_mark:                                                                 | N/A                                                                                |
| `serviceAccountBindingCount`                                                       | *number*                                                                           | :heavy_check_mark:                                                                 | N/A                                                                                |
| `serviceAccountRoles`                                                              | *string*[]                                                                         | :heavy_check_mark:                                                                 | N/A                                                                                |
| `status`                                                                           | [models.ServiceAccountHeartbeatStatus](../models/serviceaccountheartbeatstatus.md) | :heavy_check_mark:                                                                 | N/A                                                                                |
| `uniqueId`                                                                         | *string*                                                                           | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `backend`                                                                          | *"gcpServiceAccount"*                                                              | :heavy_check_mark:                                                                 | N/A                                                                                |