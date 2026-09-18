# DeploymentConfigRegistryName

## Example Usage

```typescript
import { DeploymentConfigRegistryName } from "@alienplatform/platform-api/models";

let value: DeploymentConfigRegistryName = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                        | [models.DeploymentConfigRegistryNameSecretRef](../models/deploymentconfigregistrynamesecretref.md) | :heavy_check_mark:                                                                                 | Reference to a Kubernetes Secret                                                                   |