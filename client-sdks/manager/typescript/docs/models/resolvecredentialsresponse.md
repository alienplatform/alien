# ResolveCredentialsResponse

## Example Usage

```typescript
import { ResolveCredentialsResponse } from "@alienplatform/manager-api/models";

let value: ResolveCredentialsResponse = {
  clientConfig: {
    cloud: {},
    kubernetes: {
      mode: "inCluster",
    },
    platform: "kubernetesCloud",
  },
};
```

## Fields

| Field                                              | Type                                               | Required                                           | Description                                        |
| -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- |
| `clientConfig`                                     | *models.ClientConfigUnion*                         | :heavy_check_mark:                                 | Configuration for different cloud platform clients |