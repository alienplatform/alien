# ExternalBindingLocal

Local container registry

## Example Usage

```typescript
import { ExternalBindingLocal } from "@alienplatform/manager-api/models";

let value: ExternalBindingLocal = {
  dataDir: {
    secretRef: {
      key: "<key>",
      name: "<value>",
    },
  },
  registryUrl: {
    secretRef: {
      key: "<key>",
      name: "<value>",
    },
  },
  service: "local",
  type: "artifact_registry",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `dataDir`                                                                                                            | *models.BindingValueOptionStringUnion*                                                                               | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `registryUrl`                                                                                                        | *models.BindingValueStringUnion*                                                                                     | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"local"*                                                                                                            | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.TypeArtifactRegistry4](../models/typeartifactregistry4.md)                                                   | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |