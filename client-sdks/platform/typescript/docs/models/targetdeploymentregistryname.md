# TargetDeploymentRegistryName

## Example Usage

```typescript
import { TargetDeploymentRegistryName } from "@alienplatform/platform-api/models";

let value: TargetDeploymentRegistryName = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                        | [models.TargetDeploymentRegistryNameSecretRef](../models/targetdeploymentregistrynamesecretref.md) | :heavy_check_mark:                                                                                 | Reference to a Kubernetes Secret                                                                   |