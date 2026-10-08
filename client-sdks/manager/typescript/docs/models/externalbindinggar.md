# ExternalBindingGar

Google Artifact Registry

## Example Usage

```typescript
import { ExternalBindingGar } from "@alienplatform/manager-api/models";

let value: ExternalBindingGar = {
  repositoryName: "<value>",
  service: "gar",
  type: "artifact_registry",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `pullServiceAccountEmail`                                                                                            | *models.BindingValueStringUnion*                                                                                     | :heavy_minus_sign:                                                                                                   | N/A                                                                                                                  |
| `pushServiceAccountEmail`                                                                                            | *models.BindingValueStringUnion*                                                                                     | :heavy_minus_sign:                                                                                                   | N/A                                                                                                                  |
| `repositoryName`                                                                                                     | *models.BindingValueStringUnion*                                                                                     | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"gar"*                                                                                                              | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.TypeArtifactRegistry3](../models/typeartifactregistry3.md)                                                   | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |