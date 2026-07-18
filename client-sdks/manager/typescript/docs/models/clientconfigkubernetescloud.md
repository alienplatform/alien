# ClientConfigKubernetesCloud

## Example Usage

```typescript
import { ClientConfigKubernetesCloud } from "@alienplatform/manager-api/models";

let value: ClientConfigKubernetesCloud = {
  cloud: {},
  kubernetes: {
    mode: "kubeconfig",
  },
  platform: "kubernetesCloud",
};
```

## Fields

| Field                                                                  | Type                                                                   | Required                                                               | Description                                                            |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| `cloud`                                                                | [models.Cloud](../models/cloud.md)                                     | :heavy_check_mark:                                                     | N/A                                                                    |
| `kubernetes`                                                           | *models.KubernetesClientConfig*                                        | :heavy_check_mark:                                                     | Configuration mode for Kubernetes access                               |
| `platform`                                                             | [models.PlatformKubernetesCloud](../models/platformkubernetescloud.md) | :heavy_check_mark:                                                     | N/A                                                                    |