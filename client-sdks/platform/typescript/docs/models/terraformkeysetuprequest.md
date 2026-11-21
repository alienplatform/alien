# TerraformKeySetupRequest

## Example Usage

```typescript
import { TerraformKeySetupRequest } from "@alienplatform/platform-api/models";

let value: TerraformKeySetupRequest = {
  action: "create",
  requestId: "149c01bb-2f41-4c5c-8100-60e5fa789545",
  connectedKeyDeploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  setupRoleArn: "<value>",
  deletionWindowDays: 119742,
  target: {
    operationId: "0404c6dc-a42b-4c1c-b10a-e94de27b35b4",
    deploymentName: "<value>",
    awsAccountId: "<id>",
    awsRegion: "<value>",
    resourcePrefix: "<value>",
    resourceId: "<id>",
  },
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          | Example                                                                              |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `action`                                                                             | [models.TerraformKeySetupRequestAction](../models/terraformkeysetuprequestaction.md) | :heavy_check_mark:                                                                   | N/A                                                                                  |                                                                                      |
| `requestId`                                                                          | *string*                                                                             | :heavy_check_mark:                                                                   | N/A                                                                                  |                                                                                      |
| `connectedKeyDeploymentId`                                                           | *string*                                                                             | :heavy_check_mark:                                                                   | Unique identifier for the deployment.                                                | dep_0c29fq4a2yjb7kx3smwdgxlc                                                         |
| `setupRoleArn`                                                                       | *string*                                                                             | :heavy_check_mark:                                                                   | N/A                                                                                  |                                                                                      |
| `deletionWindowDays`                                                                 | *number*                                                                             | :heavy_check_mark:                                                                   | N/A                                                                                  |                                                                                      |
| `target`                                                                             | [models.TerraformKeySetupTarget](../models/terraformkeysetuptarget.md)               | :heavy_check_mark:                                                                   | N/A                                                                                  |                                                                                      |