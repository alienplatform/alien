# SyncReconcileResponseComputeBackendHorizon

Compute backend for Container and Function resources.

Determines how compute workloads are orchestrated on cloud platforms.
When None, the platform default is used (Horizon for cloud platforms).

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

| Field                                                                                                                                                           | Type                                                                                                                                                            | Required                                                                                                                                                        | Description                                                                                                                                                     |
| --------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `clusters`                                                                                                                                                      | Record<string, [models.SyncReconcileResponseClusters](../models/syncreconcileresponseclusters.md)>                                                              | :heavy_check_mark:                                                                                                                                              | Cluster configurations (one per ContainerCluster resource)<br/>Key: ContainerCluster resource ID from stack<br/>Value: Cluster ID and management token for that cluster |
| `url`                                                                                                                                                           | *string*                                                                                                                                                        | :heavy_check_mark:                                                                                                                                              | Worker control-plane API base URL.                                                                                                                              |
| `workerImageId`                                                                                                                                                 | *string*                                                                                                                                                        | :heavy_minus_sign:                                                                                                                                              | AMI / image ID for the worker machine image.<br/><br/>The image contains the worker runtime bootstrap. Controllers only pass<br/>machine-specific settings into that image. |
| `type`                                                                                                                                                          | [models.SyncReconcileResponseComputeBackendType](../models/syncreconcileresponsecomputebackendtype.md)                                                          | :heavy_check_mark:                                                                                                                                              | N/A                                                                                                                                                             |