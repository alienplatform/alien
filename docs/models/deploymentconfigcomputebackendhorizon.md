# DeploymentConfigComputeBackendHorizon

Compute backend for Container and Worker resources.

Determines how compute workloads are orchestrated on cloud platforms.
When None, the platform default is used for cloud platforms.

## Example Usage

```typescript
import { DeploymentConfigComputeBackendHorizon } from "@alienplatform/platform-api/models";

let value: DeploymentConfigComputeBackendHorizon = {
  clusters: {
    "key": {
      clusterId: "<id>",
      managementToken: "<value>",
    },
  },
  url: "https://wealthy-adrenalin.biz/",
  type: "horizon",
};
```

## Fields

| Field                                                                                                                                                       | Type                                                                                                                                                        | Required                                                                                                                                                    | Description                                                                                                                                                 |
| ----------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `borrowedCapacityGroups`                                                                                                                                    | *string*[]                                                                                                                                                  | :heavy_minus_sign:                                                                                                                                          | Capacity groups approved for workloads in a borrowed cluster.<br/>Empty for dedicated clusters, whose groups are owned by the stack.                        |
| `clusters`                                                                                                                                                  | Record<string, [models.DeploymentConfigClusters](../models/deploymentconfigclusters.md)>                                                                    | :heavy_check_mark:                                                                                                                                          | Cluster configurations (one per ComputeCluster resource)<br/>Key: ComputeCluster resource ID from stack<br/>Value: Cluster ID and management token for that cluster |
| `horizonMachineImage`                                                                                                                                       | *models.DeploymentConfigHorizonMachineImageUnion*                                                                                                           | :heavy_minus_sign:                                                                                                                                          | N/A                                                                                                                                                         |
| `url`                                                                                                                                                       | *string*                                                                                                                                                    | :heavy_check_mark:                                                                                                                                          | Horizon control-plane API base URL.                                                                                                                         |
| `workloadNamespace`                                                                                                                                         | *string*                                                                                                                                                    | :heavy_minus_sign:                                                                                                                                          | Namespace for workloads sharing a cluster with another deployment.<br/>Absent for dedicated clusters to preserve their existing service names.              |
| `type`                                                                                                                                                      | [models.DeploymentConfigComputeBackendType](../models/deploymentconfigcomputebackendtype.md)                                                                | :heavy_check_mark:                                                                                                                                          | N/A                                                                                                                                                         |