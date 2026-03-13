# SyncAcquireResponseContainerName

## Example Usage

```typescript
import { SyncAcquireResponseContainerName } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponseContainerName = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                      | Type                                                                                                       | Required                                                                                                   | Description                                                                                                |
| ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                | [models.SyncAcquireResponseContainerNameSecretRef](../models/syncacquireresponsecontainernamesecretref.md) | :heavy_check_mark:                                                                                         | Reference to a Kubernetes Secret                                                                           |