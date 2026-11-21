# ManagerRetryResponseKubernetes2

Kubernetes runtime substrate configuration.

This controls how setup chooses the cluster backing `Platform::Kubernetes`
deployments. When omitted, cloud-backed Kubernetes deployments default to a
managed cluster and generic/on-prem Kubernetes defaults to an external
cluster.

## Example Usage

```typescript
import { ManagerRetryResponseKubernetes2 } from "@alienplatform/platform-api/models";

let value: ManagerRetryResponseKubernetes2 = {};
```

## Fields

| Field                                       | Type                                        | Required                                    | Description                                 |
| ------------------------------------------- | ------------------------------------------- | ------------------------------------------- | ------------------------------------------- |
| `cluster`                                   | *models.ManagerRetryResponseClusterUnion2*  | :heavy_minus_sign:                          | N/A                                         |
| `exposure`                                  | *models.ManagerRetryResponseExposureUnion2* | :heavy_minus_sign:                          | N/A                                         |