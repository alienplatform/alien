# ImportSourceKubernetes

Kubernetes runtime substrate configuration.

This controls how setup chooses the cluster backing `Platform::Kubernetes`
deployments. When omitted, cloud-backed Kubernetes deployments default to a
managed cluster and generic/on-prem Kubernetes defaults to an external
cluster.

## Example Usage

```typescript
import { ImportSourceKubernetes } from "@alienplatform/platform-api/models";

let value: ImportSourceKubernetes = {};
```

## Fields

| Field                              | Type                               | Required                           | Description                        |
| ---------------------------------- | ---------------------------------- | ---------------------------------- | ---------------------------------- |
| `cluster`                          | *models.ImportSourceClusterUnion*  | :heavy_minus_sign:                 | N/A                                |
| `exposure`                         | *models.ImportSourceExposureUnion* | :heavy_minus_sign:                 | N/A                                |