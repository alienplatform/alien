# SyncReconcileResponseSubscription

## Example Usage

```typescript
import { SyncReconcileResponseSubscription } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseSubscription = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                        | Type                                                                                                         | Required                                                                                                     | Description                                                                                                  |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                                  | [models.SyncReconcileResponseSubscriptionSecretRef](../models/syncreconcileresponsesubscriptionsecretref.md) | :heavy_check_mark:                                                                                           | Reference to a Kubernetes Secret                                                                             |