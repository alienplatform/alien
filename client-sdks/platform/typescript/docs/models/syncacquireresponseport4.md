# SyncAcquireResponsePort4

## Example Usage

```typescript
import { SyncAcquireResponsePort4 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponsePort4 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                | [models.SyncAcquireResponsePortSecretRef4](../models/syncacquireresponseportsecretref4.md) | :heavy_check_mark:                                                                         | Reference to a Kubernetes Secret                                                           |