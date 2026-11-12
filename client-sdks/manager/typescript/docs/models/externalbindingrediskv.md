# ExternalBindingRedisKv

Redis binding (for Kubernetes/local)

## Example Usage

```typescript
import { ExternalBindingRedisKv } from "@alienplatform/manager-api/models";

let value: ExternalBindingRedisKv = {
  connectionUrl: {
    secretRef: {
      key: "<key>",
      name: "<value>",
    },
  },
  service: "redis",
  type: "kv",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `connectionUrl`                                                                                                      | *models.BindingValueStringUnion*                                                                                     | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `database`                                                                                                           | *models.BindingValueU8Union*                                                                                         | :heavy_minus_sign:                                                                                                   | N/A                                                                                                                  |
| `keyPrefix`                                                                                                          | *models.BindingValueStringUnion*                                                                                     | :heavy_minus_sign:                                                                                                   | N/A                                                                                                                  |
| `service`                                                                                                            | *"redis"*                                                                                                            | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.TypeKv4](../models/typekv4.md)                                                                               | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
