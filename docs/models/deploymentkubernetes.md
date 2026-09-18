# DeploymentKubernetes

Kubernetes runtime substrate configuration.

This controls how setup chooses the cluster backing `Platform::Kubernetes`
deployments. When omitted, cloud-backed Kubernetes deployments default to a
managed cluster and generic/on-prem Kubernetes defaults to an external
cluster.

## Example Usage

```typescript
import { DeploymentKubernetes } from "@alienplatform/platform-api/models";

let value: DeploymentKubernetes = {};
```

## Fields

| Field                            | Type                             | Required                         | Description                      |
| -------------------------------- | -------------------------------- | -------------------------------- | -------------------------------- |
| `cluster`                        | *models.DeploymentClusterUnion*  | :heavy_minus_sign:               | N/A                              |
| `exposure`                       | *models.DeploymentExposureUnion* | :heavy_minus_sign:               | N/A                              |