# GetTerraformKeySetupOperationResponse

Key setup progress without gateway credentials

## Example Usage

```typescript
import { GetTerraformKeySetupOperationResponse } from "@alienplatform/platform-api/models/operations";

let value: GetTerraformKeySetupOperationResponse = {
  setupId: "<id>",
  operationId: "<id>",
  status: "failed",
  arn: "<value>",
  error: "<value>",
  configuration: {
    action: "delete",
    requestId: "1cd6e012-1f98-40b9-92b2-b4f6d023fb25",
    connectedKeyDeploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
    setupRoleArn: "<value>",
    deletionWindowDays: 88093,
    target: {
      operationId: "0404c6dc-a42b-4c1c-b10a-e94de27b35b4",
      deploymentName: "<value>",
      awsAccountId: "<id>",
      awsRegion: "<value>",
      resourcePrefix: "<value>",
      resourceId: "<id>",
    },
  },
};
```

## Fields

| Field                                                                                                             | Type                                                                                                              | Required                                                                                                          | Description                                                                                                       |
| ----------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| `setupId`                                                                                                         | *string*                                                                                                          | :heavy_check_mark:                                                                                                | N/A                                                                                                               |
| `operationId`                                                                                                     | *string*                                                                                                          | :heavy_check_mark:                                                                                                | N/A                                                                                                               |
| `status`                                                                                                          | [operations.GetTerraformKeySetupOperationStatus](../../models/operations/getterraformkeysetupoperationstatus.md)  | :heavy_check_mark:                                                                                                | N/A                                                                                                               |
| `arn`                                                                                                             | *string*                                                                                                          | :heavy_check_mark:                                                                                                | N/A                                                                                                               |
| `error`                                                                                                           | *string*                                                                                                          | :heavy_check_mark:                                                                                                | N/A                                                                                                               |
| `erasureStarted`                                                                                                  | *boolean*                                                                                                         | :heavy_minus_sign:                                                                                                | Key erasure has started; explicit destroy retries may continue, but the key is no longer ready for use or import. |
| `configuration`                                                                                                   | [models.TerraformKeySetupRequest](../../models/terraformkeysetuprequest.md)                                       | :heavy_minus_sign:                                                                                                | N/A                                                                                                               |