# DeploymentConfigAccountName2

## Example Usage

```typescript
import { DeploymentConfigAccountName2 } from "@alienplatform/platform-api/models";

let value: DeploymentConfigAccountName2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                        | [models.DeploymentConfigAccountNameSecretRef2](../models/deploymentconfigaccountnamesecretref2.md) | :heavy_check_mark:                                                                                 | Reference to a Kubernetes Secret                                                                   |