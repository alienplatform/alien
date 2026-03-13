# SyncAcquireResponseProjectId

## Example Usage

```typescript
import { SyncAcquireResponseProjectId } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponseProjectId = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                        | [models.SyncAcquireResponseProjectIdSecretRef](../models/syncacquireresponseprojectidsecretref.md) | :heavy_check_mark:                                                                                 | Reference to a Kubernetes Secret                                                                   |