# GetResourceDeploymentDetailResponse

Projected deployment detail for one resource.

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
  runtimeUnits: [
    {
      unitId: "<id>",
      unitKind: "<value>",
      name: "<value>",
      ready: false,
      phase: "<value>",
      nodeName: "<value>",
      restartCount: 691312,
      waitingReason: "<value>",
      terminatedReason: "<value>",
      observedAt: new Date("2026-02-11T20:30:55.689Z"),
      platformStale: true,
    },
  ],
  events: [],
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `deployment`                                                                                                         | [operations.GetResourceDeploymentDetailDeployment](../../models/operations/getresourcedeploymentdetaildeployment.md) | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `runtimeUnits`                                                                                                       | [operations.RuntimeUnit](../../models/operations/runtimeunit.md)[]                                                   | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `events`                                                                                                             | [operations.GetResourceDeploymentDetailEvent](../../models/operations/getresourcedeploymentdetailevent.md)[]         | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |