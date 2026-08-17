# DeploymentConfigAccountName1

## Example Usage

```typescript
import { DeploymentConfigAccountName1 } from "@alienplatform/platform-api/models";

let value: DeploymentConfigAccountName1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                        | [models.DeploymentConfigAccountNameSecretRef1](../models/deploymentconfigaccountnamesecretref1.md) | :heavy_check_mark:                                                                                 | Reference to a Kubernetes Secret                                                                   |