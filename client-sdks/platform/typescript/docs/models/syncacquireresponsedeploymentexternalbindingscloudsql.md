# SyncAcquireResponseDeploymentExternalBindingsCloudSQL

GCP Cloud SQL binding.

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentExternalBindingsCloudSQL } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentExternalBindingsCloudSQL = {
  service: "cloud-sql",
  type: "postgres",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `database`                                                                                                           | *models.SyncAcquireResponseDeploymentDatabaseUnion2*                                                                 | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `host`                                                                                                               | *models.SyncAcquireResponseDeploymentHostUnion1*                                                                     | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `passwordSecretName`                                                                                                 | *models.SyncAcquireResponseDeploymentPasswordSecretNameUnion*                                                        | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `port`                                                                                                               | *models.SyncAcquireResponseDeploymentPortUnion2*                                                                     | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `username`                                                                                                           | *models.SyncAcquireResponseDeploymentUsernameUnion2*                                                                 | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"cloud-sql"*                                                                                                        | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.SyncAcquireResponseDeploymentTypePostgres2](../models/syncacquireresponsedeploymenttypepostgres2.md)         | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |