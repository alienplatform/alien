# SyncAcquireResponseExternalBindingsCloudSQL

GCP Cloud SQL binding.

## Example Usage

```typescript
import { SyncAcquireResponseExternalBindingsCloudSQL } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseExternalBindingsCloudSQL = {
  service: "cloud-sql",
  type: "postgres",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `database`                                                                                                           | *models.SyncAcquireResponseDatabaseUnion2*                                                                           | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `host`                                                                                                               | *models.SyncAcquireResponseHostUnion1*                                                                               | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `passwordSecretName`                                                                                                 | *models.SyncAcquireResponsePasswordSecretNameUnion*                                                                  | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `port`                                                                                                               | *models.SyncAcquireResponsePortUnion2*                                                                               | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `username`                                                                                                           | *models.SyncAcquireResponseUsernameUnion2*                                                                           | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"cloud-sql"*                                                                                                        | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.SyncAcquireResponseTypePostgres2](../models/syncacquireresponsetypepostgres2.md)                             | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |