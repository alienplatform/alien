# SyncReconcileResponseExternalBindingsEcr

AWS ECR (Elastic Container Registry) binding configuration

## Example Usage

```typescript
import { SyncReconcileResponseExternalBindingsEcr } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseExternalBindingsEcr = {
  service: "ecr",
  type: "artifact_registry",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `pullRoleArn`                                                                                                        | *any*                                                                                                                | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `pushRoleArn`                                                                                                        | *any*                                                                                                                | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `repositoryPrefix`                                                                                                   | *models.SyncReconcileResponseRepositoryPrefixUnion*                                                                  | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"ecr"*                                                                                                              | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.SyncReconcileResponseTypeArtifactRegistry1](../models/syncreconcileresponsetypeartifactregistry1.md)         | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |