# KubernetesSettings

Kubernetes runtime substrate configuration.

This controls how setup chooses the cluster backing `Platform::Kubernetes`
deployments. When omitted, cloud-backed Kubernetes deployments default to a
managed cluster and generic/on-prem Kubernetes defaults to an external
cluster.

## Example Usage

```typescript
import { KubernetesSettings } from "@alienplatform/manager-api/models";

let value: KubernetesSettings = {};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `cluster`                                                                  | [models.KubernetesClusterSettings](../models/kubernetesclustersettings.md) | :heavy_minus_sign:                                                         | N/A                                                                        |
| `exposure`                                                                 | *models.KubernetesExposureSettings*                                        | :heavy_minus_sign:                                                         | N/A                                                                        |