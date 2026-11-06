# ExternalBindingExternal

Operator-provided / BYO database binding.

## Example Usage

```typescript
import { ExternalBindingExternal } from "@alienplatform/platform-api/models";

let value: ExternalBindingExternal = {
  database: "<value>",
  host: "rewarding-premier.biz",
  password: {
    secretRef: {
      key: "<key>",
      name: "<value>",
    },
  },
  port: "<value>",
  username: {
    "secretRef": {
      "key": "<key>",
      "name": "<value>",
    },
  },
  service: "external",
  type: "postgres",
};
```

## Fields

| Field                                                                                                                                                                                     | Type                                                                                                                                                                                      | Required                                                                                                                                                                                  | Description                                                                                                                                                                               |
| ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `database`                                                                                                                                                                                | *any*                                                                                                                                                                                     | :heavy_check_mark:                                                                                                                                                                        | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret                                                                  |
| `host`                                                                                                                                                                                    | *any*                                                                                                                                                                                     | :heavy_check_mark:                                                                                                                                                                        | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret                                                                  |
| `password`                                                                                                                                                                                | *models.PasswordUnion*                                                                                                                                                                    | :heavy_check_mark:                                                                                                                                                                        | Connection password or Kubernetes Secret reference. Kubernetes workloads resolve<br/>the reference through secretKeyRef before the application loads its binding.                         |
| `port`                                                                                                                                                                                    | *models.PortUnion5*                                                                                                                                                                       | :heavy_check_mark:                                                                                                                                                                        | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret                                                                  |
| `sslMode`                                                                                                                                                                                 | [models.SslMode](../models/sslmode.md)                                                                                                                                                    | :heavy_minus_sign:                                                                                                                                                                        | TLS policy for an operator-provided / BYO Postgres database.<br/><br/>Unlike libpq's ambiguous `prefer` mode, both choices map exactly to the<br/>connection settings exposed by every supported SDK. |
| `username`                                                                                                                                                                                | *any*                                                                                                                                                                                     | :heavy_check_mark:                                                                                                                                                                        | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret                                                                  |
| `service`                                                                                                                                                                                 | *"external"*                                                                                                                                                                              | :heavy_check_mark:                                                                                                                                                                        | N/A                                                                                                                                                                                       |
| `type`                                                                                                                                                                                    | [models.TypePostgres5](../models/typepostgres5.md)                                                                                                                                        | :heavy_check_mark:                                                                                                                                                                        | N/A                                                                                                                                                                                       |