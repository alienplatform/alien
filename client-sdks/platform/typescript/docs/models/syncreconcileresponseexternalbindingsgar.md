# SyncReconcileResponseExternalBindingsGar

Google Artifact Registry binding configuration

## Example Usage

```typescript
import { SyncReconcileResponseExternalBindingsGar } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseExternalBindingsGar = {
  service: "gar",
  type: "artifact_registry",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `pullServiceAccountEmail`                                                                                            | *any*                                                                                                                | :heavy_minus_sign:                                                                                                   | N/A                                                                                                                  |
| `pushServiceAccountEmail`                                                                                            | *any*                                                                                                                | :heavy_minus_sign:                                                                                                   | N/A                                                                                                                  |
| `repositoryName`                                                                                                     | *models.SyncReconcileResponseRepositoryNameUnion*                                                                    | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"gar"*                                                                                                              | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.TargetTypeArtifactRegistry3](../models/targettypeartifactregistry3.md)                                       | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |