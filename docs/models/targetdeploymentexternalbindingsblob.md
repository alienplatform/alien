# TargetDeploymentExternalBindingsBlob

Azure Blob Storage binding configuration

## Example Usage

```typescript
import { TargetDeploymentExternalBindingsBlob } from "@alienplatform/platform-api/models";

let value: TargetDeploymentExternalBindingsBlob = {
  service: "blob",
  type: "storage",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `accountName`                                                                                                        | *models.TargetDeploymentAccountNameUnion1*                                                                           | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `containerName`                                                                                                      | *models.TargetDeploymentContainerNameUnion*                                                                          | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"blob"*                                                                                                             | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.TargetDeploymentTypeStorage2](../models/targetdeploymenttypestorage2.md)                                     | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |