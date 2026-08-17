# TargetDeploymentServerCaCertificates

## Example Usage

```typescript
import { TargetDeploymentServerCaCertificates } from "@alienplatform/platform-api/models";

let value: TargetDeploymentServerCaCertificates = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                              | Type                                                                                                               | Required                                                                                                           | Description                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                                        | [models.TargetDeploymentServerCaCertificatesSecretRef](../models/targetdeploymentservercacertificatessecretref.md) | :heavy_check_mark:                                                                                                 | Reference to a Kubernetes Secret                                                                                   |