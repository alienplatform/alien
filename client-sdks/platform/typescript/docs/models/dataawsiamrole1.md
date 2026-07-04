# DataAwsIamRole1

## Example Usage

```typescript
import { DataAwsIamRole1 } from "@alienplatform/platform-api/models";

let value: DataAwsIamRole1 = {
  assumeRolePolicyPresent: false,
  attachedPolicyCount: 410901,
  attachedPolicyNames: [],
  createDate: "<value>",
  inlinePolicyCount: 846965,
  inlinePolicyNames: [],
  managedTagCount: 519428,
  path: "/etc",
  roleArn: "<value>",
  roleId: "<id>",
  roleName: "<value>",
  stackPermissionsApplied: false,
  status: {
    collectionIssues: [],
    health: "healthy",
    lifecycle: "deleting",
    partial: true,
    stale: false,
  },
  tagCount: 250500,
  backend: "awsIamRole",
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `assumeRolePolicyPresent`                                                  | *boolean*                                                                  | :heavy_check_mark:                                                         | N/A                                                                        |
| `attachedPolicyCount`                                                      | *number*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `attachedPolicyNames`                                                      | *string*[]                                                                 | :heavy_check_mark:                                                         | N/A                                                                        |
| `createDate`                                                               | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `description`                                                              | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `inlinePolicyCount`                                                        | *number*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `inlinePolicyNames`                                                        | *string*[]                                                                 | :heavy_check_mark:                                                         | N/A                                                                        |
| `lastUsedDate`                                                             | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `lastUsedRegion`                                                           | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `managedTagCount`                                                          | *number*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `maxSessionDuration`                                                       | *number*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `path`                                                                     | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `permissionsBoundaryArn`                                                   | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `permissionsBoundaryType`                                                  | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `roleArn`                                                                  | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `roleId`                                                                   | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `roleName`                                                                 | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `stackPermissionsApplied`                                                  | *boolean*                                                                  | :heavy_check_mark:                                                         | N/A                                                                        |
| `status`                                                                   | [models.ResourceHeartbeatStatus40](../models/resourceheartbeatstatus40.md) | :heavy_check_mark:                                                         | N/A                                                                        |
| `tagCount`                                                                 | *number*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `backend`                                                                  | *"awsIamRole"*                                                             | :heavy_check_mark:                                                         | N/A                                                                        |