# DeploymentConfigVaultPrefix1

## Example Usage

```typescript
import { DeploymentConfigVaultPrefix1 } from "@alienplatform/platform-api/models";

let value: DeploymentConfigVaultPrefix1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                        | [models.DeploymentConfigVaultPrefixSecretRef1](../models/deploymentconfigvaultprefixsecretref1.md) | :heavy_check_mark:                                                                                 | Reference to a Kubernetes Secret                                                                   |