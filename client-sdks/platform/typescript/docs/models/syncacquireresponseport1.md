# SyncAcquireResponsePort1

## Example Usage

```typescript
import { SyncAcquireResponsePort1 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponsePort1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                | [models.SyncAcquireResponsePortSecretRef1](../models/syncacquireresponseportsecretref1.md) | :heavy_check_mark:                                                                         | Reference to a Kubernetes Secret                                                           |