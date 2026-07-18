# SyncAcquireResponseDeploymentExternalBindingsBlob

Azure Blob Storage binding configuration

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentExternalBindingsBlob } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentExternalBindingsBlob = {
  service: "blob",
  type: "storage",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `accountName`                                                                                                        | *models.SyncAcquireResponseDeploymentAccountNameUnion1*                                                              | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `containerName`                                                                                                      | *models.SyncAcquireResponseDeploymentContainerNameUnion*                                                             | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"blob"*                                                                                                             | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.SyncAcquireResponseDeploymentTypeStorage2](../models/syncacquireresponsedeploymenttypestorage2.md)           | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |