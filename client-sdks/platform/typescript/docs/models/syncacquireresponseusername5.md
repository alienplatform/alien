# SyncAcquireResponseUsername5

## Example Usage

```typescript
import { SyncAcquireResponseUsername5 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseUsername5 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                        | [models.SyncAcquireResponseUsernameSecretRef5](../models/syncacquireresponseusernamesecretref5.md) | :heavy_check_mark:                                                                                 | Reference to a Kubernetes Secret                                                                   |