# TargetDeploymentAccountName1

## Example Usage

```typescript
import { TargetDeploymentAccountName1 } from "@alienplatform/platform-api/models";

let value: TargetDeploymentAccountName1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                        | [models.TargetDeploymentAccountNameSecretRef1](../models/targetdeploymentaccountnamesecretref1.md) | :heavy_check_mark:                                                                                 | Reference to a Kubernetes Secret                                                                   |