# TargetDeploymentExternalBindingsLocalPostgres

Local embedded Postgres binding.

## Example Usage

```typescript
import { TargetDeploymentExternalBindingsLocalPostgres } from "@alienplatform/platform-api/models";

let value: TargetDeploymentExternalBindingsLocalPostgres = {
  password: "3hEBvA0EFWMmzpM",
  service: "local-postgres",
  type: "postgres",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `database`                                                                                                           | *models.TargetDeploymentDatabaseUnion5*                                                                              | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `host`                                                                                                               | *models.TargetDeploymentHostUnion4*                                                                                  | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `password`                                                                                                           | *string*                                                                                                             | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `port`                                                                                                               | *models.TargetDeploymentPortUnion5*                                                                                  | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `username`                                                                                                           | *models.TargetDeploymentUsernameUnion5*                                                                              | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"local-postgres"*                                                                                                   | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.TargetDeploymentTypePostgres5](../models/targetdeploymenttypepostgres5.md)                                   | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |