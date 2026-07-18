# ClientConfigInCluster

Use in-cluster configuration (service account tokens, etc.)

## Example Usage

```typescript
import { ClientConfigInCluster } from "@alienplatform/manager-api/models";

let value: ClientConfigInCluster = {
  mode: "inCluster",
  platform: "kubernetes",
};
```

## Fields

| Field                                                                                  | Type                                                                                   | Required                                                                               | Description                                                                            |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `additionalHeaders`                                                                    | Record<string, *string*>                                                               | :heavy_minus_sign:                                                                     | Additional headers to include in requests                                              |
| `mode`                                                                                 | *"inCluster"*                                                                          | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `namespace`                                                                            | *string*                                                                               | :heavy_minus_sign:                                                                     | The namespace to operate in                                                            |
| `platform`                                                                             | [models.ClientConfigPlatformKubernetes1](../models/clientconfigplatformkubernetes1.md) | :heavy_check_mark:                                                                     | N/A                                                                                    |