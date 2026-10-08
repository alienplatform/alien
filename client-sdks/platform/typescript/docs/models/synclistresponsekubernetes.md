# SyncListResponseKubernetes

Kubernetes runtime substrate configuration.

This controls how setup chooses the cluster backing `Platform::Kubernetes`
deployments. When omitted, cloud-backed Kubernetes deployments default to a
managed cluster and generic/on-prem Kubernetes defaults to an external
cluster.

## Example Usage

```typescript
import { SyncListResponseKubernetes } from "@alienplatform/platform-api/models";

let value: SyncListResponseKubernetes = {};
```

## Fields

| Field                                  | Type                                   | Required                               | Description                            |
| -------------------------------------- | -------------------------------------- | -------------------------------------- | -------------------------------------- |
| `cluster`                              | *models.SyncListResponseClusterUnion*  | :heavy_minus_sign:                     | N/A                                    |
| `exposure`                             | *models.SyncListResponseExposureUnion* | :heavy_minus_sign:                     | N/A                                    |