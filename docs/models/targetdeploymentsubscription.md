# TargetDeploymentSubscription

## Example Usage

```typescript
import { TargetDeploymentSubscription } from "@alienplatform/platform-api/models";

let value: TargetDeploymentSubscription = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                        | [models.TargetDeploymentSubscriptionSecretRef](../models/targetdeploymentsubscriptionsecretref.md) | :heavy_check_mark:                                                                                 | Reference to a Kubernetes Secret                                                                   |