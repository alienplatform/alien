# DataAwsIamRole1

## Example Usage

```typescript
import { DataAwsIamRole1 } from "@alienplatform/platform-api/models/operations";

let value: DataAwsIamRole1 = {
  assumeRolePolicyPresent: false,
  attachedPolicyCount: 410901,
  attachedPolicyNames: [],
  createDate: "<value>",
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-06-21T12:26:29.993Z"),
      severity: "warning",
    },
  ],
  inlinePolicyCount: 87215,
  inlinePolicyNames: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
  managedTagCount: 456119,
  path: "/media",
  roleArn: "<value>",
  roleId: "<id>",
  roleName: "<value>",
  stackPermissionsApplied: false,
  status: {
    collectionIssues: [],
    health: "degraded",
    lifecycle: "updating",
    partial: false,
    stale: true,
  },
  tagCount: 813150,
  backend: "awsIamRole",
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `assumeRolePolicyPresent`                                                                                | *boolean*                                                                                                | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `attachedPolicyCount`                                                                                    | *number*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `attachedPolicyNames`                                                                                    | *string*[]                                                                                               | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `createDate`                                                                                             | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `description`                                                                                            | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `events`                                                                                                 | [operations.GetRawResourceHeartbeatEvent36](../../models/operations/getrawresourceheartbeatevent36.md)[] | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `inlinePolicyCount`                                                                                      | *number*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `inlinePolicyNames`                                                                                      | *string*[]                                                                                               | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `lastUsedDate`                                                                                           | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `lastUsedRegion`                                                                                         | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `managedTagCount`                                                                                        | *number*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `maxSessionDuration`                                                                                     | *number*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `path`                                                                                                   | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `permissionsBoundaryArn`                                                                                 | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `permissionsBoundaryType`                                                                                | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `roleArn`                                                                                                | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `roleId`                                                                                                 | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `roleName`                                                                                               | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `stackPermissionsApplied`                                                                                | *boolean*                                                                                                | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `status`                                                                                                 | [operations.DataStatus36](../../models/operations/datastatus36.md)                                       | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `tagCount`                                                                                               | *number*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `backend`                                                                                                | *"awsIamRole"*                                                                                           | :heavy_check_mark:                                                                                       | N/A                                                                                                      |