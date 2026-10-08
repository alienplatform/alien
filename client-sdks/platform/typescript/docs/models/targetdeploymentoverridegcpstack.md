# TargetDeploymentOverrideGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { TargetDeploymentOverrideGcpStack } from "@alienplatform/platform-api/models";

let value: TargetDeploymentOverrideGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                           | Type                                            | Required                                        | Description                                     |
| ----------------------------------------------- | ----------------------------------------------- | ----------------------------------------------- | ----------------------------------------------- |
| `condition`                                     | *models.TargetDeploymentOverrideConditionUnion* | :heavy_minus_sign:                              | N/A                                             |
| `scope`                                         | *string*                                        | :heavy_check_mark:                              | Scope (project/resource level)                  |