# SyncAcquireResponseExternalBindingsTablestorage

Azure Table Storage KV binding configuration

## Example Usage

```typescript
import { SyncAcquireResponseExternalBindingsTablestorage } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseExternalBindingsTablestorage = {
  service: "tablestorage",
  type: "kv",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `accountName`                                                                                                        | *models.SyncAcquireResponseAccountNameUnion2*                                                                        | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `resourceGroupName`                                                                                                  | *models.SyncAcquireResponseResourceGroupNameUnion1*                                                                  | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `tableName`                                                                                                          | *models.SyncAcquireResponseTableNameUnion2*                                                                          | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"tablestorage"*                                                                                                     | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.SyncAcquireResponseTypeKv3](../models/syncacquireresponsetypekv3.md)                                         | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |