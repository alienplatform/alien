# DeploymentConfigSubscription

## Example Usage

```typescript
import { DeploymentConfigSubscription } from "@alienplatform/platform-api/models";

let value: DeploymentConfigSubscription = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                        | [models.DeploymentConfigSubscriptionSecretRef](../models/deploymentconfigsubscriptionsecretref.md) | :heavy_check_mark:                                                                                 | Reference to a Kubernetes Secret                                                                   |