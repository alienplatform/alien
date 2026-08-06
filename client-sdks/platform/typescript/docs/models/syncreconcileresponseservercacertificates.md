# SyncReconcileResponseServerCaCertificates

## Example Usage

```typescript
import { SyncReconcileResponseServerCaCertificates } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseServerCaCertificates = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                        | Type                                                                                                                         | Required                                                                                                                     | Description                                                                                                                  |
| ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                                  | [models.SyncReconcileResponseServerCaCertificatesSecretRef](../models/syncreconcileresponseservercacertificatessecretref.md) | :heavy_check_mark:                                                                                                           | Reference to a Kubernetes Secret                                                                                             |