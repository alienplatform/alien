# SyncReconcileResponseUsername5

## Example Usage

```typescript
import { SyncReconcileResponseUsername5 } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseUsername5 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                            | [models.SyncReconcileResponseUsernameSecretRef5](../models/syncreconcileresponseusernamesecretref5.md) | :heavy_check_mark:                                                                                     | Reference to a Kubernetes Secret                                                                       |