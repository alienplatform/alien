# SyncAcquireResponseDeploymentKubernetes

Kubernetes runtime substrate configuration.

This controls how setup chooses the cluster backing `Platform::Kubernetes`
deployments. When omitted, cloud-backed Kubernetes deployments default to a
managed cluster and generic/on-prem Kubernetes defaults to an external
cluster.

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentKubernetes } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentKubernetes = {};
```

## Fields

| Field                                               | Type                                                | Required                                            | Description                                         |
| --------------------------------------------------- | --------------------------------------------------- | --------------------------------------------------- | --------------------------------------------------- |
| `cluster`                                           | *models.SyncAcquireResponseDeploymentClusterUnion*  | :heavy_minus_sign:                                  | N/A                                                 |
| `exposure`                                          | *models.SyncAcquireResponseDeploymentExposureUnion* | :heavy_minus_sign:                                  | N/A                                                 |