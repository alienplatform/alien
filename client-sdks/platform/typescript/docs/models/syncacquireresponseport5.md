# SyncAcquireResponsePort5

## Example Usage

```typescript
import { SyncAcquireResponsePort5 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponsePort5 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                | [models.SyncAcquireResponsePortSecretRef5](../models/syncacquireresponseportsecretref5.md) | :heavy_check_mark:                                                                         | Reference to a Kubernetes Secret                                                           |