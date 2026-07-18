# KubernetesClientConfigKubeconfig

Use kubeconfig file for configuration

## Example Usage

```typescript
import { KubernetesClientConfigKubeconfig } from "@alienplatform/manager-api/models";

let value: KubernetesClientConfigKubeconfig = {
  mode: "kubeconfig",
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `additionalHeaders`                                                | Record<string, *string*>                                           | :heavy_minus_sign:                                                 | Additional headers to include in requests                          |
| `cluster`                                                          | *string*                                                           | :heavy_minus_sign:                                                 | Cluster name to use (optional, defaults to context's cluster)      |
| `context`                                                          | *string*                                                           | :heavy_minus_sign:                                                 | Context name to use (optional, defaults to current-context)        |
| `kubeconfigPath`                                                   | *string*                                                           | :heavy_minus_sign:                                                 | Path to kubeconfig file (optional, defaults to standard locations) |
| `mode`                                                             | *"kubeconfig"*                                                     | :heavy_check_mark:                                                 | N/A                                                                |
| `namespace`                                                        | *string*                                                           | :heavy_minus_sign:                                                 | The namespace to operate in                                        |
| `user`                                                             | *string*                                                           | :heavy_minus_sign:                                                 | User name to use (optional, defaults to context's user)            |