# SyncAcquireResponseSubscription

## Example Usage

```typescript
import { SyncAcquireResponseSubscription } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseSubscription = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                              | [models.SyncAcquireResponseSubscriptionSecretRef](../models/syncacquireresponsesubscriptionsecretref.md) | :heavy_check_mark:                                                                                       | Reference to a Kubernetes Secret                                                                         |