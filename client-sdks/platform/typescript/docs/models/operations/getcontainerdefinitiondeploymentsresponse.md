# GetContainerDefinitionDeploymentsResponse

Per-deployment breakdown.

## Example Usage

```typescript
import { GetContainerDefinitionDeploymentsResponse } from "@alienplatform/platform-api/models/operations";

let value: GetContainerDefinitionDeploymentsResponse = {
  containerName: "<value>",
  deployments: [
    {
      clusterId: "<id>",
      deploymentId: "<id>",
      deploymentName: "<value>",
      status: "<value>",
      statusReason: "<value>",
      statusMessage: "<value>",
      image: "https://loremflickr.com/1830/714?lock=1987407703738015",
      currentReplicas: 991074,
      healthyReplicas: 660722,
      avgCpuPercent: 9526.45,
      avgMemoryPercent: 6208.12,
      avgLatencyP95Ms: 9581.94,
      avgErrorRate: 6061.6,
      avgInFlightRequests: 5018.32,
    },
  ],
};
```

## Fields

| Field                                                                                                                              | Type                                                                                                                               | Required                                                                                                                           | Description                                                                                                                        |
| ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `containerName`                                                                                                                    | *string*                                                                                                                           | :heavy_check_mark:                                                                                                                 | N/A                                                                                                                                |
| `deployments`                                                                                                                      | [operations.GetContainerDefinitionDeploymentsDeployment](../../models/operations/getcontainerdefinitiondeploymentsdeployment.md)[] | :heavy_check_mark:                                                                                                                 | N/A                                                                                                                                |