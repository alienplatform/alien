# SyncAcquireResponseQueueName

## Example Usage

```typescript
import { SyncAcquireResponseQueueName } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponseQueueName = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                        | [models.SyncAcquireResponseQueueNameSecretRef](../models/syncacquireresponsequeuenamesecretref.md) | :heavy_check_mark:                                                                                 | Reference to a Kubernetes Secret                                                                   |