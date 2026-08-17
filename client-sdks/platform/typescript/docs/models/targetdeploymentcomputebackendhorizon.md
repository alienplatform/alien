# TargetDeploymentComputeBackendHorizon

Compute backend for Container and Worker resources.

Determines how compute workloads are orchestrated on cloud platforms.
When None, the platform default is used for cloud platforms.

## Example Usage

```typescript
import { TargetDeploymentComputeBackendHorizon } from "@alienplatform/platform-api/models";

let value: TargetDeploymentComputeBackendHorizon = {
  clusters: {
    "key": {
      clusterId: "<id>",
      managementToken: "<value>",
    },
  },
  url: "https://wretched-singing.name",
  type: "horizon",
};
```

## Fields

| Field                                                                                                                                                       | Type                                                                                                                                                        | Required                                                                                                                                                    | Description                                                                                                                                                 |
| ----------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `clusters`                                                                                                                                                  | Record<string, [models.TargetDeploymentClusters](../models/targetdeploymentclusters.md)>                                                                    | :heavy_check_mark:                                                                                                                                          | Cluster configurations (one per ComputeCluster resource)<br/>Key: ComputeCluster resource ID from stack<br/>Value: Cluster ID and management token for that cluster |
| `horizonMachineImage`                                                                                                                                       | *models.TargetDeploymentHorizonMachineImageUnion*                                                                                                           | :heavy_minus_sign:                                                                                                                                          | N/A                                                                                                                                                         |
| `url`                                                                                                                                                       | *string*                                                                                                                                                    | :heavy_check_mark:                                                                                                                                          | Horizon control-plane API base URL.                                                                                                                         |
| `type`                                                                                                                                                      | [models.ComputeBackendConfigType](../models/computebackendconfigtype.md)                                                                                    | :heavy_check_mark:                                                                                                                                          | N/A                                                                                                                                                         |