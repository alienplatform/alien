# CloudFormationCallbackRequest

## Example Usage

```typescript
import { CloudFormationCallbackRequest } from "@alienplatform/platform-api/models";

let value: CloudFormationCallbackRequest = {
  stackId: "<id>",
  requestId: "<id>",
  logicalResourceId: "<id>",
  requestType: "Update",
  responseUrl: "https://only-nun.info",
  source: {
    deploymentName: "<value>",
    stackPrefix: "<value>",
    releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
    platform: "test",
    region: "<value>",
    setupTarget: "<value>",
    setupFingerprint: "<value>",
    setupFingerprintVersion: 651282,
    stackSettings: {},
    managementConfig: {
      managingRoleArn: "<value>",
      platform: "aws",
    },
    resources: [
      {
        id: "<id>",
        type: "<value>",
        importData: {
          "key": "<value>",
          "key1": "<value>",
          "key2": "<value>",
        },
      },
    ],
  },
};
```

## Fields

| Field                                          | Type                                           | Required                                       | Description                                    |
| ---------------------------------------------- | ---------------------------------------------- | ---------------------------------------------- | ---------------------------------------------- |
| `stackId`                                      | *string*                                       | :heavy_check_mark:                             | N/A                                            |
| `requestId`                                    | *string*                                       | :heavy_check_mark:                             | N/A                                            |
| `logicalResourceId`                            | *string*                                       | :heavy_check_mark:                             | N/A                                            |
| `requestType`                                  | [models.RequestType](../models/requesttype.md) | :heavy_check_mark:                             | N/A                                            |
| `responseUrl`                                  | *string*                                       | :heavy_check_mark:                             | N/A                                            |
| `physicalResourceId`                           | *string*                                       | :heavy_minus_sign:                             | N/A                                            |
| `source`                                       | [models.Source](../models/source.md)           | :heavy_minus_sign:                             | N/A                                            |
| `serviceTimeoutSeconds`                        | *number*                                       | :heavy_minus_sign:                             | N/A                                            |