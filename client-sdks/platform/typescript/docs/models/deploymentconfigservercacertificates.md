# DeploymentConfigServerCaCertificates

## Example Usage

```typescript
import { DeploymentConfigServerCaCertificates } from "@alienplatform/platform-api/models";

let value: DeploymentConfigServerCaCertificates = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                              | Type                                                                                                               | Required                                                                                                           | Description                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                                        | [models.DeploymentConfigServerCaCertificatesSecretRef](../models/deploymentconfigservercacertificatessecretref.md) | :heavy_check_mark:                                                                                                 | Reference to a Kubernetes Secret                                                                                   |