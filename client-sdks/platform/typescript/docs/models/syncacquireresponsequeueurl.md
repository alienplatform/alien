# SyncAcquireResponseQueueUrl

## Example Usage

```typescript
import { SyncAcquireResponseQueueUrl } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponseQueueUrl = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                      | [models.SyncAcquireResponseQueueUrlSecretRef](../models/syncacquireresponsequeueurlsecretref.md) | :heavy_check_mark:                                                                               | Reference to a Kubernetes Secret                                                                 |