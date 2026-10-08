# Subscription

## Example Usage

```typescript
import { Subscription } from "@alienplatform/platform-api/models";

let value: Subscription = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `secretRef`                                                        | [models.SubscriptionSecretRef](../models/subscriptionsecretref.md) | :heavy_check_mark:                                                 | Reference to a Kubernetes Secret                                   |
