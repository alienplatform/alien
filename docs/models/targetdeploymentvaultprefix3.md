# TargetDeploymentVaultPrefix3

## Example Usage

```typescript
import { TargetDeploymentVaultPrefix3 } from "@alienplatform/platform-api/models";

let value: TargetDeploymentVaultPrefix3 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                        | [models.TargetDeploymentVaultPrefixSecretRef3](../models/targetdeploymentvaultprefixsecretref3.md) | :heavy_check_mark:                                                                                 | Reference to a Kubernetes Secret                                                                   |