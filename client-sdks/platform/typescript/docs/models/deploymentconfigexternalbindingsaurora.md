# DeploymentConfigExternalBindingsAurora

AWS Aurora Serverless v2 binding.

## Example Usage

```typescript
import { DeploymentConfigExternalBindingsAurora } from "@alienplatform/platform-api/models";

let value: DeploymentConfigExternalBindingsAurora = {
  service: "aurora",
  type: "postgres",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `clusterEndpoint`                                                                                                    | *models.DeploymentConfigClusterEndpointUnion*                                                                        | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `database`                                                                                                           | *models.DeploymentConfigDatabaseUnion1*                                                                              | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `passwordSecretArn`                                                                                                  | *models.DeploymentConfigPasswordSecretArnUnion*                                                                      | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `port`                                                                                                               | *models.DeploymentConfigPortUnion1*                                                                                  | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `username`                                                                                                           | *models.DeploymentConfigUsernameUnion1*                                                                              | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"aurora"*                                                                                                           | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.DeploymentConfigTypePostgres1](../models/deploymentconfigtypepostgres1.md)                                   | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |