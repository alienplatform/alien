# TargetDeploymentExternalBindingsLocalKv

Local development KV binding configuration

## Example Usage

```typescript
import { TargetDeploymentExternalBindingsLocalKv } from "@alienplatform/platform-api/models";

let value: TargetDeploymentExternalBindingsLocalKv = {
  service: "local-kv",
  type: "kv",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `dataDir`                                                                                                            | *models.TargetDeploymentDataDirUnion1*                                                                               | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `keyPrefix`                                                                                                          | *any*                                                                                                                | :heavy_minus_sign:                                                                                                   | N/A                                                                                                                  |
| `service`                                                                                                            | *"local-kv"*                                                                                                         | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.ConfigTypeKv5](../models/configtypekv5.md)                                                                   | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |