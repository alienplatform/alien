# SyncReconcileResponseApiKey

## Example Usage

```typescript
import { SyncReconcileResponseApiKey } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseApiKey = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                      | [models.SyncReconcileResponseApiKeySecretRef](../models/syncreconcileresponseapikeysecretref.md) | :heavy_check_mark:                                                                               | Reference to a Kubernetes Secret                                                                 |