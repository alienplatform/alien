# ExternalBindingLocalKv

Local development KV binding configuration

## Example Usage

```typescript
import { ExternalBindingLocalKv } from "@alienplatform/platform-api/models";

let value: ExternalBindingLocalKv = {
  dataDir: "<value>",
  service: "local-kv",
  type: "kv",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `dataDir`                                                                                                            | *any*                                                                                                                | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `keyPrefix`                                                                                                          | *any*                                                                                                                | :heavy_minus_sign:                                                                                                   | N/A                                                                                                                  |
| `service`                                                                                                            | *"local-kv"*                                                                                                         | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.TypeKv5](../models/typekv5.md)                                                                               | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |