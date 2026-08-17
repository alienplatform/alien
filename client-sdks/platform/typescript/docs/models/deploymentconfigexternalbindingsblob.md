# DeploymentConfigExternalBindingsBlob

Azure Blob Storage binding configuration

## Example Usage

```typescript
import { DeploymentConfigExternalBindingsBlob } from "@alienplatform/platform-api/models";

let value: DeploymentConfigExternalBindingsBlob = {
  service: "blob",
  type: "storage",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `accountName`                                                                                                        | *models.DeploymentConfigAccountNameUnion1*                                                                           | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `containerName`                                                                                                      | *models.DeploymentConfigContainerNameUnion*                                                                          | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"blob"*                                                                                                             | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.DeploymentConfigTypeStorage2](../models/deploymentconfigtypestorage2.md)                                     | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |