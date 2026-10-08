# ExternalBindingAcr

Azure Container Registry binding configuration

## Example Usage

```typescript
import { ExternalBindingAcr } from "@alienplatform/platform-api/models";

let value: ExternalBindingAcr = {
  registryName: "<value>",
  resourceGroupName: "<value>",
  service: "acr",
  type: "artifact_registry",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `registryName`                                                                                                       | *any*                                                                                                                | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `repositoryPrefix`                                                                                                   | *any*                                                                                                                | :heavy_minus_sign:                                                                                                   | N/A                                                                                                                  |
| `resourceGroupName`                                                                                                  | *any*                                                                                                                | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"acr"*                                                                                                              | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.TypeArtifactRegistry2](../models/typeartifactregistry2.md)                                                   | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |