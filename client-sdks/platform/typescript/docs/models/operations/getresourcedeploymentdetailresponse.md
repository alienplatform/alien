# GetResourceDeploymentDetailResponse

Latest heartbeat detail for one compute resource deployment.

## Example Usage

```typescript
import { GetResourceDeploymentDetailResponse } from "@alienplatform/platform-api/models/operations";

let value: GetResourceDeploymentDetailResponse = {
  deployment: {
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
    partial: false,
    providerStale: true,
    platformStale: false,
    desiredCount: 644466,
    currentCount: 979572,
    readyCount: 601933,
    observedAt: new Date("2026-03-21T07:30:12.263Z"),
  },
  heartbeat: {
    status: "missing",
    deploymentId: "<id>",
    resourceId: "<id>",
    resourceType: "<value>",
  },
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `deployment`                                                                                                         | [operations.GetResourceDeploymentDetailDeployment](../../models/operations/getresourcedeploymentdetaildeployment.md) | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `heartbeat`                                                                                                          | *operations.HeartbeatUnion*                                                                                          | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |