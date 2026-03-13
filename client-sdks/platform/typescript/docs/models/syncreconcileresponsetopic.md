# SyncReconcileResponseTopic

## Example Usage

```typescript
import { SyncReconcileResponseTopic } from "@aliendotdev/platform-api/models";

let value: SyncReconcileResponseTopic = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                    | [models.SyncReconcileResponseTopicSecretRef](../models/syncreconcileresponsetopicsecretref.md) | :heavy_check_mark:                                                                             | Reference to a Kubernetes Secret                                                               |