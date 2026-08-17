# DeploymentConfigExternalBindingsCloudSQL

GCP Cloud SQL binding.

## Example Usage

```typescript
import { DeploymentConfigExternalBindingsCloudSQL } from "@alienplatform/platform-api/models";

let value: DeploymentConfigExternalBindingsCloudSQL = {
  service: "cloud-sql",
  type: "postgres",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `database`                                                                                                           | *models.DeploymentConfigDatabaseUnion2*                                                                              | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `host`                                                                                                               | *models.DeploymentConfigHostUnion1*                                                                                  | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `passwordSecretName`                                                                                                 | *models.DeploymentConfigPasswordSecretNameUnion*                                                                     | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `port`                                                                                                               | *models.DeploymentConfigPortUnion2*                                                                                  | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `serverCaCertificates`                                                                                               | *models.DeploymentConfigServerCaCertificatesUnion*                                                                   | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `username`                                                                                                           | *models.DeploymentConfigUsernameUnion2*                                                                              | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"cloud-sql"*                                                                                                        | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.DeploymentConfigTypePostgres2](../models/deploymentconfigtypepostgres2.md)                                   | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |