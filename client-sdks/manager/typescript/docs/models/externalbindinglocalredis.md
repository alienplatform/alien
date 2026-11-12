# ExternalBindingLocalRedis

Local Valkey container.

## Example Usage

```typescript
import { ExternalBindingLocalRedis } from "@alienplatform/manager-api/models";

let value: ExternalBindingLocalRedis = {
  host: {
    secretRef: {
      key: "<key>",
      name: "<value>",
    },
  },
  password: "EJO01fp6xcdr6q7",
  port: "<value>",
  username: "Kole_Stokes91",
  service: "local-redis",
  type: "redis",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `host`                                                                                                               | *models.BindingValueStringUnion*                                                                                     | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `password`                                                                                                           | *string*                                                                                                             | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `port`                                                                                                               | *models.BindingValueU16Union*                                                                                        | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `username`                                                                                                           | *models.BindingValueStringUnion*                                                                                     | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"local-redis"*                                                                                                      | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.TypeRedis5](../models/typeredis5.md)                                                                         | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
