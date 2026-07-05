# SyncReconcileResponseExternalBindingsCloudSQL

GCP Cloud SQL binding.

## Example Usage

```typescript
import { SyncReconcileResponseExternalBindingsCloudSQL } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseExternalBindingsCloudSQL = {
  service: "cloud-sql",
  type: "postgres",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `database`                                                                                                           | *models.SyncReconcileResponseDatabaseUnion2*                                                                         | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `host`                                                                                                               | *models.SyncReconcileResponseHostUnion1*                                                                             | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `passwordSecretName`                                                                                                 | *models.SyncReconcileResponsePasswordSecretNameUnion*                                                                | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `port`                                                                                                               | *models.SyncReconcileResponsePortUnion2*                                                                             | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `username`                                                                                                           | *models.SyncReconcileResponseUsernameUnion2*                                                                         | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"cloud-sql"*                                                                                                        | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.TargetTypePostgres2](../models/targettypepostgres2.md)                                                       | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |