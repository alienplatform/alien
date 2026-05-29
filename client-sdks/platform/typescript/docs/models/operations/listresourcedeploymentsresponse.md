# ListResourceDeploymentsResponse

Deployments where the resource is installed.

## Example Usage

```typescript
import { ListResourceDeploymentsResponse } from "@alienplatform/platform-api/models/operations";

let value: ListResourceDeploymentsResponse = {
  resourceType: "<value>",
  resourceId: "<id>",
  deployments: [
    {
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
      providerStale: true,
      platformStale: false,
      desiredCount: 413033,
      currentCount: 33913,
      readyCount: 404775,
      observedAt: new Date("2025-12-13T12:13:54.806Z"),
    },
  ],
};
```

## Fields

| Field                                                                                                          | Type                                                                                                           | Required                                                                                                       | Description                                                                                                    |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `resourceType`                                                                                                 | *string*                                                                                                       | :heavy_check_mark:                                                                                             | N/A                                                                                                            |
| `resourceId`                                                                                                   | *string*                                                                                                       | :heavy_check_mark:                                                                                             | N/A                                                                                                            |
| `deployments`                                                                                                  | [operations.ListResourceDeploymentsDeployment](../../models/operations/listresourcedeploymentsdeployment.md)[] | :heavy_check_mark:                                                                                             | N/A                                                                                                            |