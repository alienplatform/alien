# ExternalBindingContainerAppsEnvironment

Binding configuration for a pre-existing Azure Container Apps Environment.

Used when deploying to an existing environment instead of having Alien provision one.
This is useful for shared environments (e.g., test infrastructure) or enterprise
setups where environments are managed by a separate team.

## Example Usage

```typescript
import { ExternalBindingContainerAppsEnvironment } from "@alienplatform/platform-api/models";

let value: ExternalBindingContainerAppsEnvironment = {
  defaultDomain: "<value>",
  environmentName: "<value>",
  resourceGroupName: null,
  resourceId: "<id>",
  type: "container_apps_environment",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `defaultDomain`                                                                                                      | *any*                                                                                                                | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `environmentName`                                                                                                    | *any*                                                                                                                | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `resourceGroupName`                                                                                                  | *any*                                                                                                                | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `resourceId`                                                                                                         | *any*                                                                                                                | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `staticIp`                                                                                                           | *any*                                                                                                                | :heavy_minus_sign:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.TypeContainerAppsEnvironment](../models/typecontainerappsenvironment.md)                                     | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |