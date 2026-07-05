# SyncAcquireResponseUsername1

## Example Usage

```typescript
import { SyncAcquireResponseUsername1 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseUsername1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                        | [models.SyncAcquireResponseUsernameSecretRef1](../models/syncacquireresponseusernamesecretref1.md) | :heavy_check_mark:                                                                                 | Reference to a Kubernetes Secret                                                                   |