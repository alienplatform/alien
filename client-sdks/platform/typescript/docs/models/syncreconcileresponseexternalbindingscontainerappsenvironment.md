# SyncReconcileResponseExternalBindingsContainerAppsEnvironment

Binding configuration for a pre-existing Azure Container Apps Environment.

Used when deploying to an existing environment instead of having Alien provision one.
This is useful for shared environments (e.g., test infrastructure) or enterprise
setups where environments are managed by a separate team.

## Example Usage

```typescript
import { SyncReconcileResponseExternalBindingsContainerAppsEnvironment } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseExternalBindingsContainerAppsEnvironment = {
  type: "container_apps_environment",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `defaultDomain`                                                                                                      | *models.SyncReconcileResponseDefaultDomainUnion*                                                                     | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `environmentName`                                                                                                    | *models.SyncReconcileResponseEnvironmentNameUnion*                                                                   | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `resourceGroupName`                                                                                                  | *models.SyncReconcileResponseResourceGroupNameUnion3*                                                                | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `resourceId`                                                                                                         | *models.SyncReconcileResponseResourceIdUnion*                                                                        | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `staticIp`                                                                                                           | *any*                                                                                                                | :heavy_minus_sign:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.TargetTypeContainerAppsEnvironment](../models/targettypecontainerappsenvironment.md)                         | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |