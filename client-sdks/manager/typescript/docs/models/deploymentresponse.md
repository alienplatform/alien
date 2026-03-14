# DeploymentResponse

## Example Usage

```typescript
import { DeploymentResponse } from "@alienplatform/manager-api/models";

let value: DeploymentResponse = {
  createdAt: "1726644984201",
  deploymentGroupId: "<id>",
  id: "<id>",
  name: "<value>",
  platform: "azure",
  retryRequested: true,
  status: "<value>",
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `createdAt`                                                          | *string*                                                             | :heavy_check_mark:                                                   | N/A                                                                  |
| `currentReleaseId`                                                   | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `deploymentGroup`                                                    | [models.DeploymentGroupMinimal](../models/deploymentgroupminimal.md) | :heavy_minus_sign:                                                   | N/A                                                                  |
| `deploymentGroupId`                                                  | *string*                                                             | :heavy_check_mark:                                                   | N/A                                                                  |
| `desiredReleaseId`                                                   | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `environmentInfo`                                                    | *any*                                                                | :heavy_minus_sign:                                                   | N/A                                                                  |
| `error`                                                              | *any*                                                                | :heavy_minus_sign:                                                   | N/A                                                                  |
| `id`                                                                 | *string*                                                             | :heavy_check_mark:                                                   | N/A                                                                  |
| `name`                                                               | *string*                                                             | :heavy_check_mark:                                                   | N/A                                                                  |
| `platform`                                                           | [models.PlatformEnum](../models/platformenum.md)                     | :heavy_check_mark:                                                   | Represents the target cloud platform.                                |
| `retryRequested`                                                     | *boolean*                                                            | :heavy_check_mark:                                                   | N/A                                                                  |
| `stackSettings`                                                      | *any*                                                                | :heavy_minus_sign:                                                   | N/A                                                                  |
| `stackState`                                                         | *any*                                                                | :heavy_minus_sign:                                                   | N/A                                                                  |
| `status`                                                             | *string*                                                             | :heavy_check_mark:                                                   | N/A                                                                  |
| `updatedAt`                                                          | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |