# TargetDeploymentExternalBindingsAcr

Azure Container Registry binding configuration

## Example Usage

```typescript
import { TargetDeploymentExternalBindingsAcr } from "@alienplatform/platform-api/models";

let value: TargetDeploymentExternalBindingsAcr = {
  service: "acr",
  type: "artifact_registry",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `registryName`                                                                                                       | *models.TargetDeploymentRegistryNameUnion*                                                                           | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `repositoryPrefix`                                                                                                   | *any*                                                                                                                | :heavy_minus_sign:                                                                                                   | N/A                                                                                                                  |
| `resourceGroupName`                                                                                                  | *models.TargetDeploymentResourceGroupNameUnion2*                                                                     | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"acr"*                                                                                                              | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.TargetDeploymentTypeArtifactRegistry2](../models/targetdeploymenttypeartifactregistry2.md)                   | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |