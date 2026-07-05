# SyncAcquireResponseDatabase1

## Example Usage

```typescript
import { SyncAcquireResponseDatabase1 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDatabase1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                        | [models.SyncAcquireResponseDatabaseSecretRef1](../models/syncacquireresponsedatabasesecretref1.md) | :heavy_check_mark:                                                                                 | Reference to a Kubernetes Secret                                                                   |