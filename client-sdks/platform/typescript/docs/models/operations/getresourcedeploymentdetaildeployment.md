# GetResourceDeploymentDetailDeployment

## Example Usage

```typescript
import { GetResourceDeploymentDetailDeployment } from "@alienplatform/platform-api/models/operations";

let value: GetResourceDeploymentDetailDeployment = {
  deploymentId: "<id>",
  deploymentName: "<value>",
  deploymentGroupId: "<id>",
  deploymentGroupName: "<value>",
  resourceType: "<value>",
  resourceId: "<id>",
  backend: "<value>",
  controllerPlatform: "<value>",
  health: "<value>",
  lifecycle: "<value>",
  message: "<value>",
  partial: true,
  providerStale: false,
  platformStale: true,
  desiredCount: null,
  currentCount: 894151,
  readyCount: 664200,
  observedAt: new Date("2026-05-21T17:39:43.812Z"),
  desiredImage: "<value>",
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `deploymentId`                                                                                | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `deploymentName`                                                                              | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `deploymentGroupId`                                                                           | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `deploymentGroupName`                                                                         | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `resourceType`                                                                                | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `resourceId`                                                                                  | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `backend`                                                                                     | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `controllerPlatform`                                                                          | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `health`                                                                                      | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `lifecycle`                                                                                   | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `message`                                                                                     | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `partial`                                                                                     | *boolean*                                                                                     | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `providerStale`                                                                               | *boolean*                                                                                     | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `platformStale`                                                                               | *boolean*                                                                                     | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `desiredCount`                                                                                | *number*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `currentCount`                                                                                | *number*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `readyCount`                                                                                  | *number*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `observedAt`                                                                                  | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `desiredImage`                                                                                | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |