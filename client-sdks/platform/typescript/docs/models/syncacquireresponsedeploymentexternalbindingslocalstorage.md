# SyncAcquireResponseDeploymentExternalBindingsLocalStorage

Local filesystem storage binding configuration

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentExternalBindingsLocalStorage } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentExternalBindingsLocalStorage = {
  service: "local-storage",
  type: "storage",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `storagePath`                                                                                                        | *models.SyncAcquireResponseDeploymentStoragePathUnion*                                                               | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"local-storage"*                                                                                                    | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.SyncAcquireResponseDeploymentTypeStorage4](../models/syncacquireresponsedeploymenttypestorage4.md)           | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |