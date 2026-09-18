# TargetDeploymentVaultPrefix1

## Example Usage

```typescript
import { TargetDeploymentVaultPrefix1 } from "@alienplatform/platform-api/models";

let value: TargetDeploymentVaultPrefix1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                        | [models.TargetDeploymentVaultPrefixSecretRef1](../models/targetdeploymentvaultprefixsecretref1.md) | :heavy_check_mark:                                                                                 | Reference to a Kubernetes Secret                                                                   |