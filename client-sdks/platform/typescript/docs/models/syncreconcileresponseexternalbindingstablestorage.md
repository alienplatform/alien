# SyncReconcileResponseExternalBindingsTablestorage

Azure Table Storage KV binding configuration

## Example Usage

```typescript
import { SyncReconcileResponseExternalBindingsTablestorage } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseExternalBindingsTablestorage = {
  service: "tablestorage",
  type: "kv",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `accountName`                                                                                                        | *models.SyncReconcileResponseAccountNameUnion2*                                                                      | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `resourceGroupName`                                                                                                  | *models.SyncReconcileResponseResourceGroupNameUnion1*                                                                | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `tableName`                                                                                                          | *models.SyncReconcileResponseTableNameUnion2*                                                                        | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"tablestorage"*                                                                                                     | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.SyncReconcileResponseTypeKv3](../models/syncreconcileresponsetypekv3.md)                                     | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |