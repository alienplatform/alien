# SyncAcquireResponseExternalBindingsBlob

Azure Blob Storage binding configuration

## Example Usage

```typescript
import { SyncAcquireResponseExternalBindingsBlob } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseExternalBindingsBlob = {
  service: "blob",
  type: "storage",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `accountName`                                                                                                        | *models.SyncAcquireResponseAccountNameUnion1*                                                                        | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `containerName`                                                                                                      | *models.SyncAcquireResponseContainerNameUnion*                                                                       | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"blob"*                                                                                                             | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.SyncAcquireResponseTypeStorage2](../models/syncacquireresponsetypestorage2.md)                               | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |