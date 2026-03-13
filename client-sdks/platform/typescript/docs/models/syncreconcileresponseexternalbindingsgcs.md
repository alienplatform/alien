# SyncReconcileResponseExternalBindingsGcs

Google Cloud Storage binding configuration

## Example Usage

```typescript
import { SyncReconcileResponseExternalBindingsGcs } from "@aliendotdev/platform-api/models";

let value: SyncReconcileResponseExternalBindingsGcs = {
  service: "gcs",
  type: "storage",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `bucketName`                                                                                                         | *any*                                                                                                                | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"gcs"*                                                                                                              | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.SyncReconcileResponseTypeStorage3](../models/syncreconcileresponsetypestorage3.md)                           | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |