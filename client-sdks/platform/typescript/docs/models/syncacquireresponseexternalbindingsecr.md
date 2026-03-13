# SyncAcquireResponseExternalBindingsEcr

AWS ECR (Elastic Container Registry) binding configuration

## Example Usage

```typescript
import { SyncAcquireResponseExternalBindingsEcr } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponseExternalBindingsEcr = {
  service: "ecr",
  type: "artifact_registry",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `pullRoleArn`                                                                                                        | *any*                                                                                                                | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `pushRoleArn`                                                                                                        | *any*                                                                                                                | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `repositoryPrefix`                                                                                                   | *any*                                                                                                                | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"ecr"*                                                                                                              | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.SyncAcquireResponseTypeArtifactRegistry1](../models/syncacquireresponsetypeartifactregistry1.md)             | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |