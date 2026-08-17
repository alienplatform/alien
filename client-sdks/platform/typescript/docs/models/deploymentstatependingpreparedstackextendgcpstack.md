# DeploymentStatePendingPreparedStackExtendGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { DeploymentStatePendingPreparedStackExtendGcpStack } from "@alienplatform/platform-api/models";

let value: DeploymentStatePendingPreparedStackExtendGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                                 | Type                                                                  | Required                                                              | Description                                                           |
| --------------------------------------------------------------------- | --------------------------------------------------------------------- | --------------------------------------------------------------------- | --------------------------------------------------------------------- |
| `condition`                                                           | *models.DeploymentStatePendingPreparedStackExtendStackConditionUnion* | :heavy_minus_sign:                                                    | N/A                                                                   |
| `scope`                                                               | *string*                                                              | :heavy_check_mark:                                                    | Scope (project/resource level)                                        |