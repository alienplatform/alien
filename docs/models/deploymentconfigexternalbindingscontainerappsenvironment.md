# DeploymentConfigExternalBindingsContainerAppsEnvironment

Binding configuration for a pre-existing Azure Container Apps Environment.

Used when deploying to an existing environment instead of having Alien provision one.
This is useful for shared environments (e.g., test infrastructure) or enterprise
setups where environments are managed by a separate team.

## Example Usage

```typescript
import { DeploymentConfigExternalBindingsContainerAppsEnvironment } from "@alienplatform/platform-api/models";

let value: DeploymentConfigExternalBindingsContainerAppsEnvironment = {
  type: "container_apps_environment",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `defaultDomain`                                                                                                      | *models.DeploymentConfigDefaultDomainUnion*                                                                          | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `environmentName`                                                                                                    | *models.DeploymentConfigEnvironmentNameUnion*                                                                        | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `resourceGroupName`                                                                                                  | *models.DeploymentConfigResourceGroupNameUnion3*                                                                     | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `resourceId`                                                                                                         | *models.DeploymentConfigResourceIdUnion*                                                                             | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `staticIp`                                                                                                           | *any*                                                                                                                | :heavy_minus_sign:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.DeploymentConfigTypeContainerAppsEnvironment](../models/deploymentconfigtypecontainerappsenvironment.md)     | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |