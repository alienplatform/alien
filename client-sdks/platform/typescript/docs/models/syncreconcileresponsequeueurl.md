# SyncReconcileResponseQueueUrl

## Example Usage

```typescript
import { SyncReconcileResponseQueueUrl } from "@aliendotdev/platform-api/models";

let value: SyncReconcileResponseQueueUrl = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                          | [models.SyncReconcileResponseQueueUrlSecretRef](../models/syncreconcileresponsequeueurlsecretref.md) | :heavy_check_mark:                                                                                   | Reference to a Kubernetes Secret                                                                     |