# PrepareDeploymentStackKubernetes

Kubernetes runtime substrate configuration.

This controls how setup chooses the cluster backing `Platform::Kubernetes`
deployments. When omitted, cloud-backed Kubernetes deployments default to a
managed cluster and generic/on-prem Kubernetes defaults to an external
cluster.

## Example Usage

```typescript
import { PrepareDeploymentStackKubernetes } from "@alienplatform/platform-api/models/operations";

let value: PrepareDeploymentStackKubernetes = {};
```

## Fields

| Field                                            | Type                                             | Required                                         | Description                                      |
| ------------------------------------------------ | ------------------------------------------------ | ------------------------------------------------ | ------------------------------------------------ |
| `cluster`                                        | *operations.PrepareDeploymentStackClusterUnion*  | :heavy_minus_sign:                               | N/A                                              |
| `exposure`                                       | *operations.PrepareDeploymentStackExposureUnion* | :heavy_minus_sign:                               | N/A                                              |