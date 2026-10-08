# ExternalBindingTablestorage

Azure Table Storage KV binding configuration

## Example Usage

```typescript
import { ExternalBindingTablestorage } from "@alienplatform/platform-api/models";

let value: ExternalBindingTablestorage = {
  accountName: "<value>",
  resourceGroupName: "<value>",
  tableName: {
    "secretRef": {
      "key": "<key>",
      "name": "<value>",
    },
  },
  service: "tablestorage",
  type: "kv",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `accountName`                                                                                                        | *any*                                                                                                                | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `resourceGroupName`                                                                                                  | *any*                                                                                                                | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `tableName`                                                                                                          | *any*                                                                                                                | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"tablestorage"*                                                                                                     | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.TypeKv3](../models/typekv3.md)                                                                               | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |