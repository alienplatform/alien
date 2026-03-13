# SyncReconcileResponseExternalBindingsLocalStorage

Local filesystem storage binding configuration

## Example Usage

```typescript
import { SyncReconcileResponseExternalBindingsLocalStorage } from "@aliendotdev/platform-api/models";

let value: SyncReconcileResponseExternalBindingsLocalStorage = {
  service: "local-storage",
  type: "storage",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `storagePath`                                                                                                        | *any*                                                                                                                | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"local-storage"*                                                                                                    | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.SyncReconcileResponseTypeStorage4](../models/syncreconcileresponsetypestorage4.md)                           | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |