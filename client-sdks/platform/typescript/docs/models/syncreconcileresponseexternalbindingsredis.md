# SyncReconcileResponseExternalBindingsRedis

Redis KV binding configuration

## Example Usage

```typescript
import { SyncReconcileResponseExternalBindingsRedis } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseExternalBindingsRedis = {
  service: "redis",
  type: "kv",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `connectionUrl`                                                                                                      | *models.SyncReconcileResponseConnectionUrlUnion*                                                                     | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `database`                                                                                                           | *any*                                                                                                                | :heavy_minus_sign:                                                                                                   | N/A                                                                                                                  |
| `keyPrefix`                                                                                                          | *any*                                                                                                                | :heavy_minus_sign:                                                                                                   | N/A                                                                                                                  |
| `service`                                                                                                            | *"redis"*                                                                                                            | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.SyncReconcileResponseTypeKv4](../models/syncreconcileresponsetypekv4.md)                                     | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |