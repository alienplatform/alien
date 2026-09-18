# ManagerRetryResponseKubernetes3

Kubernetes runtime substrate configuration.

This controls how setup chooses the cluster backing `Platform::Kubernetes`
deployments. When omitted, cloud-backed Kubernetes deployments default to a
managed cluster and generic/on-prem Kubernetes defaults to an external
cluster.

## Example Usage

```typescript
import { ManagerRetryResponseKubernetes3 } from "@alienplatform/platform-api/models";

let value: ManagerRetryResponseKubernetes3 = {};
```

## Fields

| Field                                       | Type                                        | Required                                    | Description                                 |
| ------------------------------------------- | ------------------------------------------- | ------------------------------------------- | ------------------------------------------- |
| `cluster`                                   | *models.ManagerRetryResponseClusterUnion3*  | :heavy_minus_sign:                          | N/A                                         |
| `exposure`                                  | *models.ManagerRetryResponseExposureUnion3* | :heavy_minus_sign:                          | N/A                                         |