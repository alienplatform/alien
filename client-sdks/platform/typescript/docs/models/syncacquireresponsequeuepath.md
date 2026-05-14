# SyncAcquireResponseQueuePath

## Example Usage

```typescript
import { SyncAcquireResponseQueuePath } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseQueuePath = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                        | [models.SyncAcquireResponseQueuePathSecretRef](../models/syncacquireresponsequeuepathsecretref.md) | :heavy_check_mark:                                                                                 | Reference to a Kubernetes Secret                                                                   |