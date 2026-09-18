# DeploymentConfigVaultPrefix3

## Example Usage

```typescript
import { DeploymentConfigVaultPrefix3 } from "@alienplatform/platform-api/models";

let value: DeploymentConfigVaultPrefix3 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                        | [models.DeploymentConfigVaultPrefixSecretRef3](../models/deploymentconfigvaultprefixsecretref3.md) | :heavy_check_mark:                                                                                 | Reference to a Kubernetes Secret                                                                   |