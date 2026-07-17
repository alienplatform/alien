# SyncAcquireResponseDeploymentSubscription

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentSubscription } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentSubscription = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                        | Type                                                                                                                         | Required                                                                                                                     | Description                                                                                                                  |
| ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                                  | [models.SyncAcquireResponseDeploymentSubscriptionSecretRef](../models/syncacquireresponsedeploymentsubscriptionsecretref.md) | :heavy_check_mark:                                                                                                           | Reference to a Kubernetes Secret                                                                                             |