# TargetDeploymentVaultPrefix2

## Example Usage

```typescript
import { TargetDeploymentVaultPrefix2 } from "@alienplatform/platform-api/models";

let value: TargetDeploymentVaultPrefix2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                        | [models.TargetDeploymentVaultPrefixSecretRef2](../models/targetdeploymentvaultprefixsecretref2.md) | :heavy_check_mark:                                                                                 | Reference to a Kubernetes Secret                                                                   |