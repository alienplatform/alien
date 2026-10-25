# ExternalBindingAuroraManaged

Aurora-managed JSON credentials. Separate tag makes older readers reject
this binding rather than treating the JSON document as a raw password.

## Example Usage

```typescript
import { ExternalBindingAuroraManaged } from "@alienplatform/manager-api/models";

let value: ExternalBindingAuroraManaged = {
  clusterEndpoint: {
    secretRef: {
      key: "<key>",
      name: "<value>",
    },
  },
  database: "<value>",
  passwordSecretArn: "<value>",
  port: "<value>",
  username: {
    secretRef: {
      key: "<key>",
      name: "<value>",
    },
  },
  service: "aurora-managed",
  type: "postgres",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `clusterEndpoint`                                                                                                    | *models.BindingValueStringUnion*                                                                                     | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `database`                                                                                                           | *models.BindingValueStringUnion*                                                                                     | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `passwordSecretArn`                                                                                                  | *models.BindingValueStringUnion*                                                                                     | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `port`                                                                                                               | *models.BindingValueU16Union*                                                                                        | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `username`                                                                                                           | *models.BindingValueStringUnion*                                                                                     | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"aurora-managed"*                                                                                                   | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.TypePostgres2](../models/typepostgres2.md)                                                                   | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
