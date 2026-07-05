# SyncReconcileResponsePasswordSecretArn

## Example Usage

```typescript
import { SyncReconcileResponsePasswordSecretArn } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponsePasswordSecretArn = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                  | Type                                                                                                                   | Required                                                                                                               | Description                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                            | [models.SyncReconcileResponsePasswordSecretArnSecretRef](../models/syncreconcileresponsepasswordsecretarnsecretref.md) | :heavy_check_mark:                                                                                                     | Reference to a Kubernetes Secret                                                                                       |