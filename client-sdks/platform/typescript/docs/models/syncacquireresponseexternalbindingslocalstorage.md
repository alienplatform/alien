# SyncAcquireResponseExternalBindingsLocalStorage

Local filesystem storage binding configuration

## Example Usage

```typescript
import { SyncAcquireResponseExternalBindingsLocalStorage } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponseExternalBindingsLocalStorage = {
  service: "local-storage",
  type: "storage",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `storagePath`                                                                                                        | *any*                                                                                                                | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"local-storage"*                                                                                                    | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.SyncAcquireResponseTypeStorage4](../models/syncacquireresponsetypestorage4.md)                               | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |