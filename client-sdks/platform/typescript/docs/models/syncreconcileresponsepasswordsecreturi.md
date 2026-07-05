# SyncReconcileResponsePasswordSecretUri

## Example Usage

```typescript
import { SyncReconcileResponsePasswordSecretUri } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponsePasswordSecretUri = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                  | Type                                                                                                                   | Required                                                                                                               | Description                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                            | [models.SyncReconcileResponsePasswordSecretUriSecretRef](../models/syncreconcileresponsepasswordsecreturisecretref.md) | :heavy_check_mark:                                                                                                     | Reference to a Kubernetes Secret                                                                                       |