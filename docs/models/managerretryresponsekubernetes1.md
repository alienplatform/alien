# ManagerRetryResponseKubernetes1

Kubernetes runtime substrate configuration.

This controls how setup chooses the cluster backing `Platform::Kubernetes`
deployments. When omitted, cloud-backed Kubernetes deployments default to a
managed cluster and generic/on-prem Kubernetes defaults to an external
cluster.

## Example Usage

```typescript
import { ManagerRetryResponseKubernetes1 } from "@alienplatform/platform-api/models";

let value: ManagerRetryResponseKubernetes1 = {};
```

## Fields

| Field                                       | Type                                        | Required                                    | Description                                 |
| ------------------------------------------- | ------------------------------------------- | ------------------------------------------- | ------------------------------------------- |
| `cluster`                                   | *models.ManagerRetryResponseClusterUnion1*  | :heavy_minus_sign:                          | N/A                                         |
| `exposure`                                  | *models.ManagerRetryResponseExposureUnion1* | :heavy_minus_sign:                          | N/A                                         |