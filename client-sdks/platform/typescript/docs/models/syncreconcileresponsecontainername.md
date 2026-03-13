# SyncReconcileResponseContainerName

## Example Usage

```typescript
import { SyncReconcileResponseContainerName } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseContainerName = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                          | Type                                                                                                           | Required                                                                                                       | Description                                                                                                    |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                    | [models.SyncReconcileResponseContainerNameSecretRef](../models/syncreconcileresponsecontainernamesecretref.md) | :heavy_check_mark:                                                                                             | Reference to a Kubernetes Secret                                                                               |