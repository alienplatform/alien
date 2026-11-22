# DeploymentInfoKubernetes

Kubernetes runtime substrate configuration.

This controls how setup chooses the cluster backing `Platform::Kubernetes`
deployments. When omitted, cloud-backed Kubernetes deployments default to a
managed cluster and generic/on-prem Kubernetes defaults to an external
cluster.

## Example Usage

```typescript
import { DeploymentInfoKubernetes } from "@alienplatform/platform-api/models";

let value: DeploymentInfoKubernetes = {};
```

## Fields

| Field                                | Type                                 | Required                             | Description                          |
| ------------------------------------ | ------------------------------------ | ------------------------------------ | ------------------------------------ |
| `cluster`                            | *models.DeploymentInfoClusterUnion*  | :heavy_minus_sign:                   | N/A                                  |
| `exposure`                           | *models.DeploymentInfoExposureUnion* | :heavy_minus_sign:                   | N/A                                  |