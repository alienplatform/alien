# SyncReconcileResponseExternalBindingsFlexibleServer

Azure Flexible Server binding.

## Example Usage

```typescript
import { SyncReconcileResponseExternalBindingsFlexibleServer } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseExternalBindingsFlexibleServer = {
  service: "flexible-server",
  type: "postgres",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `database`                                                                                                           | *models.SyncReconcileResponseDatabaseUnion3*                                                                         | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `host`                                                                                                               | *models.SyncReconcileResponseHostUnion2*                                                                             | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `passwordSecretUri`                                                                                                  | *models.SyncReconcileResponsePasswordSecretUriUnion*                                                                 | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `port`                                                                                                               | *models.SyncReconcileResponsePortUnion3*                                                                             | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `username`                                                                                                           | *models.SyncReconcileResponseUsernameUnion3*                                                                         | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"flexible-server"*                                                                                                  | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.TargetTypePostgres3](../models/targettypepostgres3.md)                                                       | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |