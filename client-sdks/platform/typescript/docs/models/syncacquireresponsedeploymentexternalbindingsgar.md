# SyncAcquireResponseDeploymentExternalBindingsGar

Google Artifact Registry binding configuration

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentExternalBindingsGar } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentExternalBindingsGar = {
  service: "gar",
  type: "artifact_registry",
};
```

## Fields

| Field                                                                                                                        | Type                                                                                                                         | Required                                                                                                                     | Description                                                                                                                  |
| ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `pullServiceAccountEmail`                                                                                                    | *any*                                                                                                                        | :heavy_minus_sign:                                                                                                           | N/A                                                                                                                          |
| `pushServiceAccountEmail`                                                                                                    | *any*                                                                                                                        | :heavy_minus_sign:                                                                                                           | N/A                                                                                                                          |
| `repositoryName`                                                                                                             | *models.SyncAcquireResponseDeploymentRepositoryNameUnion*                                                                    | :heavy_minus_sign:                                                                                                           | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret     |
| `service`                                                                                                                    | *"gar"*                                                                                                                      | :heavy_check_mark:                                                                                                           | N/A                                                                                                                          |
| `type`                                                                                                                       | [models.SyncAcquireResponseDeploymentTypeArtifactRegistry3](../models/syncacquireresponsedeploymenttypeartifactregistry3.md) | :heavy_check_mark:                                                                                                           | N/A                                                                                                                          |