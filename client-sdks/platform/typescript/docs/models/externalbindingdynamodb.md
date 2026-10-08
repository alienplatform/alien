# ExternalBindingDynamodb

AWS DynamoDB KV binding configuration

## Example Usage

```typescript
import { ExternalBindingDynamodb } from "@alienplatform/platform-api/models";

let value: ExternalBindingDynamodb = {
  region: {
    "secretRef": {
      "key": "<key>",
      "name": "<value>",
    },
  },
  tableName: "<value>",
  service: "dynamodb",
  type: "kv",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `endpointUrl`                                                                                                        | *any*                                                                                                                | :heavy_minus_sign:                                                                                                   | N/A                                                                                                                  |
| `region`                                                                                                             | *any*                                                                                                                | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `tableName`                                                                                                          | *any*                                                                                                                | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"dynamodb"*                                                                                                         | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.TypeKv1](../models/typekv1.md)                                                                               | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
