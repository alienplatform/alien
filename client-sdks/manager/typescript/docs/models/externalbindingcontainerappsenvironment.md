# ExternalBindingContainerAppsEnvironment

External Azure Container Apps Environment binding (pre-existing environment)

## Example Usage

```typescript
import { ExternalBindingContainerAppsEnvironment } from "@alienplatform/manager-api/models";

let value: ExternalBindingContainerAppsEnvironment = {
  defaultDomain: "<value>",
  environmentName: {
    secretRef: {
      key: "<key>",
      name: "<value>",
    },
  },
  resourceGroupName: "<value>",
  resourceId: {
    secretRef: {
      key: "<key>",
      name: "<value>",
    },
  },
  type: "container_apps_environment",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `defaultDomain`                                                                                                      | *models.BindingValueStringUnion*                                                                                     | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `environmentName`                                                                                                    | *models.BindingValueStringUnion*                                                                                     | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `resourceGroupName`                                                                                                  | *models.BindingValueStringUnion*                                                                                     | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `resourceId`                                                                                                         | *models.BindingValueStringUnion*                                                                                     | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `staticIp`                                                                                                           | *models.BindingValueStringUnion*                                                                                     | :heavy_minus_sign:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.TypeContainerAppsEnvironment](../models/typecontainerappsenvironment.md)                                     | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |