# CreateManagerResponseKubernetes1

Kubernetes runtime substrate configuration.

This controls how setup chooses the cluster backing `Platform::Kubernetes`
deployments. When omitted, cloud-backed Kubernetes deployments default to a
managed cluster and generic/on-prem Kubernetes defaults to an external
cluster.

## Example Usage

```typescript
import { CreateManagerResponseKubernetes1 } from "@alienplatform/platform-api/models";

let value: CreateManagerResponseKubernetes1 = {};
```

## Fields

| Field                                        | Type                                         | Required                                     | Description                                  |
| -------------------------------------------- | -------------------------------------------- | -------------------------------------------- | -------------------------------------------- |
| `cluster`                                    | *models.CreateManagerResponseClusterUnion1*  | :heavy_minus_sign:                           | N/A                                          |
| `exposure`                                   | *models.CreateManagerResponseExposureUnion1* | :heavy_minus_sign:                           | N/A                                          |