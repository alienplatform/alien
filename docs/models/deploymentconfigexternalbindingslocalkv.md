# DeploymentConfigExternalBindingsLocalKv

Local development KV binding configuration

## Example Usage

```typescript
import { DeploymentConfigExternalBindingsLocalKv } from "@alienplatform/platform-api/models";

let value: DeploymentConfigExternalBindingsLocalKv = {
  service: "local-kv",
  type: "kv",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `dataDir`                                                                                                            | *models.DeploymentConfigDataDirUnion1*                                                                               | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `keyPrefix`                                                                                                          | *any*                                                                                                                | :heavy_minus_sign:                                                                                                   | N/A                                                                                                                  |
| `service`                                                                                                            | *"local-kv"*                                                                                                         | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.DeploymentConfigTypeKv5](../models/deploymentconfigtypekv5.md)                                               | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |