# KubernetesClusterSettings

Kubernetes cluster setup settings.

## Example Usage

```typescript
import { KubernetesClusterSettings } from "@alienplatform/manager-api/models";

let value: KubernetesClusterSettings = {
  ownership: "external",
};
```

## Fields

| Field                                                                        | Type                                                                         | Required                                                                     | Description                                                                  |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `cloud`                                                                      | [models.KubernetesCloudReference](../models/kubernetescloudreference.md)     | :heavy_minus_sign:                                                           | N/A                                                                          |
| `namespace`                                                                  | *string*                                                                     | :heavy_minus_sign:                                                           | Namespace where the Alien chart and application resources run.               |
| `ownership`                                                                  | [models.KubernetesClusterOwnership](../models/kubernetesclusterownership.md) | :heavy_check_mark:                                                           | Ownership model for the Kubernetes cluster.                                  |