# SyncAcquireResponseHost3

## Example Usage

```typescript
import { SyncAcquireResponseHost3 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseHost3 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                | [models.SyncAcquireResponseHostSecretRef3](../models/syncacquireresponsehostsecretref3.md) | :heavy_check_mark:                                                                         | Reference to a Kubernetes Secret                                                           |