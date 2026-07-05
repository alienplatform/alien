# SyncAcquireResponseUsername2

## Example Usage

```typescript
import { SyncAcquireResponseUsername2 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseUsername2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                        | [models.SyncAcquireResponseUsernameSecretRef2](../models/syncacquireresponseusernamesecretref2.md) | :heavy_check_mark:                                                                                 | Reference to a Kubernetes Secret                                                                   |