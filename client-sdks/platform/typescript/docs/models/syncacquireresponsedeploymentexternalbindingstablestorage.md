# SyncAcquireResponseDeploymentExternalBindingsTablestorage

Azure Table Storage KV binding configuration

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentExternalBindingsTablestorage } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentExternalBindingsTablestorage = {
  service: "tablestorage",
  type: "kv",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `accountName`                                                                                                        | *models.SyncAcquireResponseDeploymentAccountNameUnion2*                                                              | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `resourceGroupName`                                                                                                  | *models.SyncAcquireResponseDeploymentResourceGroupNameUnion1*                                                        | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `tableName`                                                                                                          | *models.SyncAcquireResponseDeploymentTableNameUnion2*                                                                | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"tablestorage"*                                                                                                     | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.SyncAcquireResponseDeploymentTypeKv3](../models/syncacquireresponsedeploymenttypekv3.md)                     | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |