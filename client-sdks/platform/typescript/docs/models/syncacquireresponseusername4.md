# SyncAcquireResponseUsername4

## Example Usage

```typescript
import { SyncAcquireResponseUsername4 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseUsername4 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                        | [models.SyncAcquireResponseUsernameSecretRef4](../models/syncacquireresponseusernamesecretref4.md) | :heavy_check_mark:                                                                                 | Reference to a Kubernetes Secret                                                                   |