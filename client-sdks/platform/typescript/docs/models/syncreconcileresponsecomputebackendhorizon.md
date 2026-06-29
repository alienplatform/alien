# SyncReconcileResponseComputeBackendHorizon

Compute backend for Container and Worker resources.

Determines how compute workloads are orchestrated on cloud platforms.
When None, the platform default is used for cloud platforms.

## Example Usage

```typescript
import { SyncReconcileResponseComputeBackendHorizon } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseComputeBackendHorizon = {
  clusters: {},
  url: "https://alarmed-effector.info/",
  type: "horizon",
};
```

## Fields

| Field                                                                                                                                                       | Type                                                                                                                                                        | Required                                                                                                                                                    | Description                                                                                                                                                 |
| ----------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `clusters`                                                                                                                                                  | Record<string, [models.SyncReconcileResponseClusters](../models/syncreconcileresponseclusters.md)>                                                          | :heavy_check_mark:                                                                                                                                          | Cluster configurations (one per ComputeCluster resource)<br/>Key: ComputeCluster resource ID from stack<br/>Value: Cluster ID and management token for that cluster |
| `horizonMachineImage`                                                                                                                                       | *models.SyncReconcileResponseHorizonMachineImageUnion*                                                                                                      | :heavy_minus_sign:                                                                                                                                          | N/A                                                                                                                                                         |
| `url`                                                                                                                                                       | *string*                                                                                                                                                    | :heavy_check_mark:                                                                                                                                          | Horizon control-plane API base URL.                                                                                                                         |
| `type`                                                                                                                                                      | [models.ComputeBackendTargetType](../models/computebackendtargettype.md)                                                                                    | :heavy_check_mark:                                                                                                                                          | N/A                                                                                                                                                         |