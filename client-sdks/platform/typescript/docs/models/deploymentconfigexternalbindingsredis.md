# DeploymentConfigExternalBindingsRedis

Redis KV binding configuration

## Example Usage

```typescript
import { DeploymentConfigExternalBindingsRedis } from "@alienplatform/platform-api/models";

let value: DeploymentConfigExternalBindingsRedis = {
  service: "redis",
  type: "kv",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `connectionUrl`                                                                                                      | *models.DeploymentConfigConnectionUrlUnion*                                                                          | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `database`                                                                                                           | *any*                                                                                                                | :heavy_minus_sign:                                                                                                   | N/A                                                                                                                  |
| `keyPrefix`                                                                                                          | *any*                                                                                                                | :heavy_minus_sign:                                                                                                   | N/A                                                                                                                  |
| `service`                                                                                                            | *"redis"*                                                                                                            | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.DeploymentConfigTypeKv4](../models/deploymentconfigtypekv4.md)                                               | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |