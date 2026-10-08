# PlanDeploymentComputeKubernetes

Kubernetes runtime substrate configuration.

This controls how setup chooses the cluster backing `Platform::Kubernetes`
deployments. When omitted, cloud-backed Kubernetes deployments default to a
managed cluster and generic/on-prem Kubernetes defaults to an external
cluster.

## Example Usage

```typescript
import { PlanDeploymentComputeKubernetes } from "@alienplatform/platform-api/models/operations";

let value: PlanDeploymentComputeKubernetes = {};
```

## Fields

| Field                                           | Type                                            | Required                                        | Description                                     |
| ----------------------------------------------- | ----------------------------------------------- | ----------------------------------------------- | ----------------------------------------------- |
| `cluster`                                       | *operations.PlanDeploymentComputeClusterUnion*  | :heavy_minus_sign:                              | N/A                                             |
| `exposure`                                      | *operations.PlanDeploymentComputeExposureUnion* | :heavy_minus_sign:                              | N/A                                             |