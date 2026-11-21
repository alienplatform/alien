# TargetDeploymentExtendGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { TargetDeploymentExtendGcpStack } from "@alienplatform/platform-api/models";

let value: TargetDeploymentExtendGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                         | Type                                          | Required                                      | Description                                   |
| --------------------------------------------- | --------------------------------------------- | --------------------------------------------- | --------------------------------------------- |
| `condition`                                   | *models.TargetDeploymentExtendConditionUnion* | :heavy_minus_sign:                            | N/A                                           |
| `scope`                                       | *string*                                      | :heavy_check_mark:                            | Scope (project/resource level)                |