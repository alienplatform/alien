# SyncReconcileResponseExternalBindingsDynamodb

AWS DynamoDB KV binding configuration

## Example Usage

```typescript
import { SyncReconcileResponseExternalBindingsDynamodb } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseExternalBindingsDynamodb = {
  service: "dynamodb",
  type: "kv",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `endpointUrl`                                                                                                        | *any*                                                                                                                | :heavy_minus_sign:                                                                                                   | N/A                                                                                                                  |
| `region`                                                                                                             | *models.SyncReconcileResponseRegionUnion*                                                                            | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `tableName`                                                                                                          | *models.SyncReconcileResponseTableNameUnion1*                                                                        | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"dynamodb"*                                                                                                         | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.SyncReconcileResponseTypeKv1](../models/syncreconcileresponsetypekv1.md)                                     | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |