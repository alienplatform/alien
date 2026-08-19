# DeploymentConfigVaultPrefix2

## Example Usage

```typescript
import { DeploymentConfigVaultPrefix2 } from "@alienplatform/platform-api/models";

let value: DeploymentConfigVaultPrefix2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                        | [models.DeploymentConfigVaultPrefixSecretRef2](../models/deploymentconfigvaultprefixsecretref2.md) | :heavy_check_mark:                                                                                 | Reference to a Kubernetes Secret                                                                   |