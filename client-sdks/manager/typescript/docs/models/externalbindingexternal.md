# ExternalBindingExternal

Operator-provided / BYO database (Kubernetes, on-prem, or cloud override).

## Example Usage

```typescript
import { ExternalBindingExternal } from "@alienplatform/manager-api/models";

let value: ExternalBindingExternal = {
  database: {
    secretRef: {
      key: "<key>",
      name: "<value>",
    },
  },
  host: "yellow-handover.net",
  password: {
    secretKeyRef: {
      key: "<key>",
      name: "<value>",
    },
  },
  port: 106185,
  username: "Wiley_Ortiz",
  service: "external",
  type: "postgres",
};
```

## Fields

| Field                                                                                                                                                                                     | Type                                                                                                                                                                                      | Required                                                                                                                                                                                  | Description                                                                                                                                                                               |
| ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `database`                                                                                                                                                                                | *models.BindingValueStringUnion*                                                                                                                                                          | :heavy_check_mark:                                                                                                                                                                        | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret                                                                  |
| `host`                                                                                                                                                                                    | *models.BindingValueStringUnion*                                                                                                                                                          | :heavy_check_mark:                                                                                                                                                                        | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret                                                                  |
| `password`                                                                                                                                                                                | *models.Password2*                                                                                                                                                                        | :heavy_check_mark:                                                                                                                                                                        | Connection password or Kubernetes Secret reference. Kubernetes workloads resolve<br/>the reference through secretKeyRef before the application loads its binding.                         |
| `port`                                                                                                                                                                                    | *models.BindingValueU16Union*                                                                                                                                                             | :heavy_check_mark:                                                                                                                                                                        | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret                                                                  |
| `sslMode`                                                                                                                                                                                 | [models.ExternalPostgresSslMode](../models/externalpostgressslmode.md)                                                                                                                    | :heavy_minus_sign:                                                                                                                                                                        | TLS policy for an operator-provided / BYO Postgres database.<br/><br/>Unlike libpq's ambiguous `prefer` mode, both choices map exactly to the<br/>connection settings exposed by every supported SDK. |
| `username`                                                                                                                                                                                | *models.BindingValueStringUnion*                                                                                                                                                          | :heavy_check_mark:                                                                                                                                                                        | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret                                                                  |
| `service`                                                                                                                                                                                 | *"external"*                                                                                                                                                                              | :heavy_check_mark:                                                                                                                                                                        | N/A                                                                                                                                                                                       |
| `type`                                                                                                                                                                                    | [models.TypePostgres5](../models/typepostgres5.md)                                                                                                                                        | :heavy_check_mark:                                                                                                                                                                        | N/A                                                                                                                                                                                       |
