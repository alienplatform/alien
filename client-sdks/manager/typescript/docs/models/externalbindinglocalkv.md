# ExternalBindingLocalKv

Local development KV (for testing)

## Example Usage

```typescript
import { ExternalBindingLocalKv } from "@alienplatform/manager-api/models";

let value: ExternalBindingLocalKv = {
  dataDir: {
    secretRef: {
      key: "<key>",
      name: "<value>",
    },
  },
  service: "local-kv",
  type: "kv",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `dataDir`                                                                                                            | *models.BindingValueStringUnion*                                                                                     | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `keyPrefix`                                                                                                          | *models.BindingValueStringUnion*                                                                                     | :heavy_minus_sign:                                                                                                   | N/A                                                                                                                  |
| `service`                                                                                                            | *"local-kv"*                                                                                                         | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.TypeKv5](../models/typekv5.md)                                                                               | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |