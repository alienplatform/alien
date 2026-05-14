# SyncReconcileResponseQueuePath

## Example Usage

```typescript
import { SyncReconcileResponseQueuePath } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseQueuePath = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                            | [models.SyncReconcileResponseQueuePathSecretRef](../models/syncreconcileresponsequeuepathsecretref.md) | :heavy_check_mark:                                                                                     | Reference to a Kubernetes Secret                                                                       |