# TargetDeploymentExternalBindingsContainerAppsEnvironment

Binding configuration for a pre-existing Azure Container Apps Environment.

Used when deploying to an existing environment instead of having Alien provision one.
This is useful for shared environments (e.g., test infrastructure) or enterprise
setups where environments are managed by a separate team.

## Example Usage

```typescript
import { TargetDeploymentExternalBindingsContainerAppsEnvironment } from "@alienplatform/platform-api/models";

let value: TargetDeploymentExternalBindingsContainerAppsEnvironment = {
  type: "container_apps_environment",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `defaultDomain`                                                                                                      | *models.TargetDeploymentDefaultDomainUnion*                                                                          | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `environmentName`                                                                                                    | *models.TargetDeploymentEnvironmentNameUnion*                                                                        | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `resourceGroupName`                                                                                                  | *models.TargetDeploymentResourceGroupNameUnion3*                                                                     | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `resourceId`                                                                                                         | *models.TargetDeploymentResourceIdUnion*                                                                             | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `staticIp`                                                                                                           | *any*                                                                                                                | :heavy_minus_sign:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.ConfigTypeContainerAppsEnvironment](../models/configtypecontainerappsenvironment.md)                         | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |