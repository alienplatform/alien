# ExternalBindingEcr

AWS ECR (Elastic Container Registry) binding configuration

## Example Usage

```typescript
import { ExternalBindingEcr } from "@alienplatform/platform-api/models";

let value: ExternalBindingEcr = {
  repositoryPrefix: {
    "secretRef": {
      "key": "<key>",
      "name": "<value>",
    },
  },
  service: "ecr",
  type: "artifact_registry",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `pullRoleArn`                                                                                                        | *any*                                                                                                                | :heavy_minus_sign:                                                                                                   | N/A                                                                                                                  |
| `pushRoleArn`                                                                                                        | *any*                                                                                                                | :heavy_minus_sign:                                                                                                   | N/A                                                                                                                  |
| `repositoryPrefix`                                                                                                   | *any*                                                                                                                | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"ecr"*                                                                                                              | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.TypeArtifactRegistry1](../models/typeartifactregistry1.md)                                                   | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |