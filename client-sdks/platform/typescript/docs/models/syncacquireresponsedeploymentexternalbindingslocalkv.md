# SyncAcquireResponseDeploymentExternalBindingsLocalKv

Local development KV binding configuration

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentExternalBindingsLocalKv } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentExternalBindingsLocalKv = {
  service: "local-kv",
  type: "kv",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `dataDir`                                                                                                            | *models.SyncAcquireResponseDeploymentDataDirUnion1*                                                                  | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `keyPrefix`                                                                                                          | *any*                                                                                                                | :heavy_minus_sign:                                                                                                   | N/A                                                                                                                  |
| `service`                                                                                                            | *"local-kv"*                                                                                                         | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.SyncAcquireResponseDeploymentTypeKv5](../models/syncacquireresponsedeploymenttypekv5.md)                     | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |