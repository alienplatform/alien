# SyncAcquireResponseDeploymentExternalBindingsAcr

Azure Container Registry binding configuration

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentExternalBindingsAcr } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentExternalBindingsAcr = {
  service: "acr",
  type: "artifact_registry",
};
```

## Fields

| Field                                                                                                                        | Type                                                                                                                         | Required                                                                                                                     | Description                                                                                                                  |
| ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `registryName`                                                                                                               | *models.SyncAcquireResponseDeploymentRegistryNameUnion*                                                                      | :heavy_minus_sign:                                                                                                           | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret     |
| `repositoryPrefix`                                                                                                           | *any*                                                                                                                        | :heavy_minus_sign:                                                                                                           | N/A                                                                                                                          |
| `resourceGroupName`                                                                                                          | *models.SyncAcquireResponseDeploymentResourceGroupNameUnion2*                                                                | :heavy_minus_sign:                                                                                                           | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret     |
| `service`                                                                                                                    | *"acr"*                                                                                                                      | :heavy_check_mark:                                                                                                           | N/A                                                                                                                          |
| `type`                                                                                                                       | [models.SyncAcquireResponseDeploymentTypeArtifactRegistry2](../models/syncacquireresponsedeploymenttypeartifactregistry2.md) | :heavy_check_mark:                                                                                                           | N/A                                                                                                                          |