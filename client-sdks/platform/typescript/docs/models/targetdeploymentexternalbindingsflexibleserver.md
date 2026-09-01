# TargetDeploymentExternalBindingsFlexibleServer

Azure Flexible Server binding.

## Example Usage

```typescript
import { TargetDeploymentExternalBindingsFlexibleServer } from "@alienplatform/platform-api/models";

let value: TargetDeploymentExternalBindingsFlexibleServer = {
  service: "flexible-server",
  type: "postgres",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `database`                                                                                                           | *models.TargetDeploymentDatabaseUnion3*                                                                              | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `host`                                                                                                               | *models.TargetDeploymentHostUnion2*                                                                                  | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `passwordSecretUri`                                                                                                  | *models.TargetDeploymentPasswordSecretUriUnion*                                                                      | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `port`                                                                                                               | *models.TargetDeploymentPortUnion3*                                                                                  | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `username`                                                                                                           | *models.TargetDeploymentUsernameUnion3*                                                                              | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"flexible-server"*                                                                                                  | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.TargetDeploymentTypePostgres3](../models/targetdeploymenttypepostgres3.md)                                   | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |