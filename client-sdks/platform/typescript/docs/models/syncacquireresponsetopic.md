# SyncAcquireResponseTopic

## Example Usage

```typescript
import { SyncAcquireResponseTopic } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseTopic = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                | [models.SyncAcquireResponseTopicSecretRef](../models/syncacquireresponsetopicsecretref.md) | :heavy_check_mark:                                                                         | Reference to a Kubernetes Secret                                                           |