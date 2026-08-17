# TargetDeploymentExternalBindingsTablestorage

Azure Table Storage KV binding configuration

## Example Usage

```typescript
import { TargetDeploymentExternalBindingsTablestorage } from "@alienplatform/platform-api/models";

let value: TargetDeploymentExternalBindingsTablestorage = {
  service: "tablestorage",
  type: "kv",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `accountName`                                                                                                        | *models.TargetDeploymentAccountNameUnion2*                                                                           | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `resourceGroupName`                                                                                                  | *models.TargetDeploymentResourceGroupNameUnion1*                                                                     | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `tableName`                                                                                                          | *models.TargetDeploymentTableNameUnion2*                                                                             | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"tablestorage"*                                                                                                     | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.ConfigTypeKv3](../models/configtypekv3.md)                                                                   | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |