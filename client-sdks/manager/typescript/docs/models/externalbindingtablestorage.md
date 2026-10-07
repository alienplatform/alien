# ExternalBindingTablestorage

Azure Table Storage binding

## Example Usage

```typescript
import { ExternalBindingTablestorage } from "@alienplatform/manager-api/models";

let value: ExternalBindingTablestorage = {
  accountName: {
    secretRef: {
      key: "<key>",
      name: "<value>",
    },
  },
  resourceGroupName: "<value>",
  tableName: "<value>",
  service: "tablestorage",
  type: "kv",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `accountName`                                                                                                        | *models.BindingValueStringUnion*                                                                                     | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `resourceGroupName`                                                                                                  | *models.BindingValueStringUnion*                                                                                     | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `tableName`                                                                                                          | *models.BindingValueStringUnion*                                                                                     | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"tablestorage"*                                                                                                     | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.TypeKv3](../models/typekv3.md)                                                                               | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |