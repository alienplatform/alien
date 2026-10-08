# TargetDeploymentExtendGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { TargetDeploymentExtendGcpResource } from "@alienplatform/platform-api/models";

let value: TargetDeploymentExtendGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                 | Type                                                  | Required                                              | Description                                           |
| ----------------------------------------------------- | ----------------------------------------------------- | ----------------------------------------------------- | ----------------------------------------------------- |
| `condition`                                           | *models.TargetDeploymentExtendResourceConditionUnion* | :heavy_minus_sign:                                    | N/A                                                   |
| `scope`                                               | *string*                                              | :heavy_check_mark:                                    | Scope (project/resource level)                        |