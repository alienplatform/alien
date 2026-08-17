# DeploymentConfigExternalBindingsLocalPostgres

Local embedded Postgres binding.

## Example Usage

```typescript
import { DeploymentConfigExternalBindingsLocalPostgres } from "@alienplatform/platform-api/models";

let value: DeploymentConfigExternalBindingsLocalPostgres = {
  password: "VvdP7qKtXHlFBzh",
  service: "local-postgres",
  type: "postgres",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `database`                                                                                                           | *models.DeploymentConfigDatabaseUnion5*                                                                              | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `host`                                                                                                               | *models.DeploymentConfigHostUnion4*                                                                                  | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `password`                                                                                                           | *string*                                                                                                             | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `port`                                                                                                               | *models.DeploymentConfigPortUnion5*                                                                                  | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `username`                                                                                                           | *models.DeploymentConfigUsernameUnion5*                                                                              | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"local-postgres"*                                                                                                   | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.DeploymentConfigTypePostgres5](../models/deploymentconfigtypepostgres5.md)                                   | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |