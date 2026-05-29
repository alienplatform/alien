# ServiceAccountHeartbeatDataAwsIamRole

## Example Usage

```typescript
import { ServiceAccountHeartbeatDataAwsIamRole } from "@alienplatform/manager-api/models";

let value: ServiceAccountHeartbeatDataAwsIamRole = {
  assumeRolePolicyPresent: true,
  attachedPolicyCount: 703796,
  attachedPolicyNames: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
  createDate: "<value>",
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-02-23T02:52:34.144Z"),
      severity: "info",
    },
  ],
  inlinePolicyCount: 792146,
  inlinePolicyNames: [
    "<value 1>",
    "<value 2>",
  ],
  managedTagCount: 874935,
  path: "/var/log",
  roleArn: "<value>",
  roleId: "<id>",
  roleName: "<value>",
  stackPermissionsApplied: true,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "deleting",
    partial: true,
    stale: false,
  },
  tagCount: 961527,
  backend: "awsIamRole",
};
```

## Fields

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `assumeRolePolicyPresent`                                                          | *boolean*                                                                          | :heavy_check_mark:                                                                 | N/A                                                                                |
| `attachedPolicyCount`                                                              | *number*                                                                           | :heavy_check_mark:                                                                 | N/A                                                                                |
| `attachedPolicyNames`                                                              | *string*[]                                                                         | :heavy_check_mark:                                                                 | N/A                                                                                |
| `createDate`                                                                       | *string*                                                                           | :heavy_check_mark:                                                                 | N/A                                                                                |
| `description`                                                                      | *string*                                                                           | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `events`                                                                           | [models.HeartbeatEvent](../models/heartbeatevent.md)[]                             | :heavy_check_mark:                                                                 | N/A                                                                                |
| `inlinePolicyCount`                                                                | *number*                                                                           | :heavy_check_mark:                                                                 | N/A                                                                                |
| `inlinePolicyNames`                                                                | *string*[]                                                                         | :heavy_check_mark:                                                                 | N/A                                                                                |
| `lastUsedDate`                                                                     | *string*                                                                           | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `lastUsedRegion`                                                                   | *string*                                                                           | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `managedTagCount`                                                                  | *number*                                                                           | :heavy_check_mark:                                                                 | N/A                                                                                |
| `maxSessionDuration`                                                               | *number*                                                                           | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `path`                                                                             | *string*                                                                           | :heavy_check_mark:                                                                 | N/A                                                                                |
| `permissionsBoundaryArn`                                                           | *string*                                                                           | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `permissionsBoundaryType`                                                          | *string*                                                                           | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `roleArn`                                                                          | *string*                                                                           | :heavy_check_mark:                                                                 | N/A                                                                                |
| `roleId`                                                                           | *string*                                                                           | :heavy_check_mark:                                                                 | N/A                                                                                |
| `roleName`                                                                         | *string*                                                                           | :heavy_check_mark:                                                                 | N/A                                                                                |
| `stackPermissionsApplied`                                                          | *boolean*                                                                          | :heavy_check_mark:                                                                 | N/A                                                                                |
| `status`                                                                           | [models.ServiceAccountHeartbeatStatus](../models/serviceaccountheartbeatstatus.md) | :heavy_check_mark:                                                                 | N/A                                                                                |
| `tagCount`                                                                         | *number*                                                                           | :heavy_check_mark:                                                                 | N/A                                                                                |
| `backend`                                                                          | *"awsIamRole"*                                                                     | :heavy_check_mark:                                                                 | N/A                                                                                |