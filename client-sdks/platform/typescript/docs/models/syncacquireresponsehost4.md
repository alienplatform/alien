# SyncAcquireResponseHost4

## Example Usage

```typescript
import { SyncAcquireResponseHost4 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseHost4 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                | [models.SyncAcquireResponseHostSecretRef4](../models/syncacquireresponsehostsecretref4.md) | :heavy_check_mark:                                                                         | Reference to a Kubernetes Secret                                                           |