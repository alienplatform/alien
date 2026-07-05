# SyncAcquireResponseExternalBindingsLocalPostgres

Local embedded Postgres binding.

## Example Usage

```typescript
import { SyncAcquireResponseExternalBindingsLocalPostgres } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseExternalBindingsLocalPostgres = {
  password: "nBiWyJC0vNY0RXy",
  service: "local-postgres",
  type: "postgres",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `database`                                                                                                           | *models.SyncAcquireResponseDatabaseUnion5*                                                                           | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `host`                                                                                                               | *models.SyncAcquireResponseHostUnion4*                                                                               | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `password`                                                                                                           | *string*                                                                                                             | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `port`                                                                                                               | *models.SyncAcquireResponsePortUnion5*                                                                               | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `username`                                                                                                           | *models.SyncAcquireResponseUsernameUnion5*                                                                           | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"local-postgres"*                                                                                                   | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.SyncAcquireResponseTypePostgres5](../models/syncacquireresponsetypepostgres5.md)                             | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |