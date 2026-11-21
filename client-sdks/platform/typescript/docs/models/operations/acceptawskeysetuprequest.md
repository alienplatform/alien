# AcceptAwsKeySetupRequest

## Example Usage

```typescript
import { AcceptAwsKeySetupRequest } from "@alienplatform/platform-api/models/operations";

let value: AcceptAwsKeySetupRequest = {
  action: "delete",
  connectedKeyDeploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  nativeKeyLogicalId: "<id>",
  setupRoleArn: "<value>",
  deletionWindowDays: 893781,
  cloudFormation: {
    stackId: "<id>",
    requestId: "<id>",
    logicalResourceId: "<id>",
    physicalResourceId: "<id>",
    responseUrl: "https://clear-fencing.biz",
  },
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              | Example                                                                                                  |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `action`                                                                                                 | [operations.Action](../../models/operations/action.md)                                                   | :heavy_check_mark:                                                                                       | N/A                                                                                                      |                                                                                                          |
| `connectedKeyDeploymentId`                                                                               | *string*                                                                                                 | :heavy_check_mark:                                                                                       | Unique identifier for the deployment.                                                                    | dep_0c29fq4a2yjb7kx3smwdgxlc                                                                             |
| `nativeKeyLogicalId`                                                                                     | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |                                                                                                          |
| `setupRoleArn`                                                                                           | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |                                                                                                          |
| `deletionWindowDays`                                                                                     | *number*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |                                                                                                          |
| `cloudFormation`                                                                                         | [operations.AcceptAwsKeySetupCloudFormation](../../models/operations/acceptawskeysetupcloudformation.md) | :heavy_check_mark:                                                                                       | N/A                                                                                                      |                                                                                                          |