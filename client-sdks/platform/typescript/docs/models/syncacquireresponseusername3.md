# SyncAcquireResponseUsername3

## Example Usage

```typescript
import { SyncAcquireResponseUsername3 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseUsername3 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                        | [models.SyncAcquireResponseUsernameSecretRef3](../models/syncacquireresponseusernamesecretref3.md) | :heavy_check_mark:                                                                                 | Reference to a Kubernetes Secret                                                                   |