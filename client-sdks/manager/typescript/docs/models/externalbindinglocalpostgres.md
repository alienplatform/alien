# ExternalBindingLocalPostgres

Local embedded Postgres process.

## Example Usage

```typescript
import { ExternalBindingLocalPostgres } from "@alienplatform/manager-api/models";

let value: ExternalBindingLocalPostgres = {
  database: {
    secretRef: {
      key: "<key>",
      name: "<value>",
    },
  },
  host: "unruly-hubris.info",
  password: "LkLoz0Nj2fUYzUZ",
  port: 273248,
  username: {
    secretRef: {
      key: "<key>",
      name: "<value>",
    },
  },
  service: "local-postgres",
  type: "postgres",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `database`                                                                                                           | *models.BindingValueStringUnion*                                                                                     | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `host`                                                                                                               | *models.BindingValueStringUnion*                                                                                     | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `password`                                                                                                           | *string*                                                                                                             | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `port`                                                                                                               | *models.BindingValueU16Union*                                                                                        | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `username`                                                                                                           | *models.BindingValueStringUnion*                                                                                     | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"local-postgres"*                                                                                                   | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.TypePostgres5](../models/typepostgres5.md)                                                                   | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |