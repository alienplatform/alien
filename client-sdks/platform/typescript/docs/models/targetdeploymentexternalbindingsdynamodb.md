# TargetDeploymentExternalBindingsDynamodb

AWS DynamoDB KV binding configuration

## Example Usage

```typescript
import { TargetDeploymentExternalBindingsDynamodb } from "@alienplatform/platform-api/models";

let value: TargetDeploymentExternalBindingsDynamodb = {
  service: "dynamodb",
  type: "kv",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `endpointUrl`                                                                                                        | *any*                                                                                                                | :heavy_minus_sign:                                                                                                   | N/A                                                                                                                  |
| `region`                                                                                                             | *models.TargetDeploymentRegionUnion*                                                                                 | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `tableName`                                                                                                          | *models.TargetDeploymentTableNameUnion1*                                                                             | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"dynamodb"*                                                                                                         | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.ConfigTypeKv1](../models/configtypekv1.md)                                                                   | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |