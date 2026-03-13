# SyncAcquireResponseExternalBindingsLocalKv

Local development KV binding configuration

## Example Usage

```typescript
import { SyncAcquireResponseExternalBindingsLocalKv } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponseExternalBindingsLocalKv = {
  service: "local-kv",
  type: "kv",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `dataDir`                                                                                                            | *any*                                                                                                                | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `keyPrefix`                                                                                                          | *any*                                                                                                                | :heavy_minus_sign:                                                                                                   | N/A                                                                                                                  |
| `service`                                                                                                            | *"local-kv"*                                                                                                         | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.SyncAcquireResponseTypeKv5](../models/syncacquireresponsetypekv5.md)                                         | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |