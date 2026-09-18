# DeploymentConfigExternalBindingsEcr

AWS ECR (Elastic Container Registry) binding configuration

## Example Usage

```typescript
import { DeploymentConfigExternalBindingsEcr } from "@alienplatform/platform-api/models";

let value: DeploymentConfigExternalBindingsEcr = {
  service: "ecr",
  type: "artifact_registry",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `pullRoleArn`                                                                                                        | *any*                                                                                                                | :heavy_minus_sign:                                                                                                   | N/A                                                                                                                  |
| `pushRoleArn`                                                                                                        | *any*                                                                                                                | :heavy_minus_sign:                                                                                                   | N/A                                                                                                                  |
| `repositoryPrefix`                                                                                                   | *models.DeploymentConfigRepositoryPrefixUnion*                                                                       | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"ecr"*                                                                                                              | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.DeploymentConfigTypeArtifactRegistry1](../models/deploymentconfigtypeartifactregistry1.md)                   | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |