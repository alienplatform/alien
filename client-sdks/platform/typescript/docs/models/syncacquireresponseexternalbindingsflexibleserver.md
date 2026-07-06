# SyncAcquireResponseExternalBindingsFlexibleServer

Azure Flexible Server binding.

## Example Usage

```typescript
import { SyncAcquireResponseExternalBindingsFlexibleServer } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseExternalBindingsFlexibleServer = {
  service: "flexible-server",
  type: "postgres",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `database`                                                                                                           | *models.SyncAcquireResponseDatabaseUnion3*                                                                           | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `host`                                                                                                               | *models.SyncAcquireResponseHostUnion2*                                                                               | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `passwordSecretUri`                                                                                                  | *models.SyncAcquireResponsePasswordSecretUriUnion*                                                                   | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `port`                                                                                                               | *models.SyncAcquireResponsePortUnion3*                                                                               | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `username`                                                                                                           | *models.SyncAcquireResponseUsernameUnion3*                                                                           | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"flexible-server"*                                                                                                  | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.SyncAcquireResponseTypePostgres3](../models/syncacquireresponsetypepostgres3.md)                             | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |