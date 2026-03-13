# SyncReconcileResponseQueueName

## Example Usage

```typescript
import { SyncReconcileResponseQueueName } from "@aliendotdev/platform-api/models";

let value: SyncReconcileResponseQueueName = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                            | [models.SyncReconcileResponseQueueNameSecretRef](../models/syncreconcileresponsequeuenamesecretref.md) | :heavy_check_mark:                                                                                     | Reference to a Kubernetes Secret                                                                       |