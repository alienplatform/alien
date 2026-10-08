# ExternalBindingRedis

Redis KV binding configuration

## Example Usage

```typescript
import { ExternalBindingRedis } from "@alienplatform/platform-api/models";

let value: ExternalBindingRedis = {
  connectionUrl: "https://smoggy-consistency.info",
  service: "redis",
  type: "kv",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `connectionUrl`                                                                                                      | *any*                                                                                                                | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `database`                                                                                                           | *models.DatabaseUnion*                                                                                               | :heavy_minus_sign:                                                                                                   | N/A                                                                                                                  |
| `keyPrefix`                                                                                                          | *any*                                                                                                                | :heavy_minus_sign:                                                                                                   | N/A                                                                                                                  |
| `service`                                                                                                            | *"redis"*                                                                                                            | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.TypeKv4](../models/typekv4.md)                                                                               | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
