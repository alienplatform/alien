# TargetDeploymentAccountName2

## Example Usage

```typescript
import { TargetDeploymentAccountName2 } from "@alienplatform/platform-api/models";

let value: TargetDeploymentAccountName2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                        | [models.TargetDeploymentAccountNameSecretRef2](../models/targetdeploymentaccountnamesecretref2.md) | :heavy_check_mark:                                                                                 | Reference to a Kubernetes Secret                                                                   |