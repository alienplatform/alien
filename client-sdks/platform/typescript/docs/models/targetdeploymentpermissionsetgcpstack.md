# TargetDeploymentPermissionSetGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { TargetDeploymentPermissionSetGcpStack } from "@alienplatform/platform-api/models";

let value: TargetDeploymentPermissionSetGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                   | Type                                                    | Required                                                | Description                                             |
| ------------------------------------------------------- | ------------------------------------------------------- | ------------------------------------------------------- | ------------------------------------------------------- |
| `condition`                                             | *models.TargetDeploymentPermissionSetGcpConditionUnion* | :heavy_minus_sign:                                      | N/A                                                     |
| `scope`                                                 | *string*                                                | :heavy_check_mark:                                      | Scope (project/resource level)                          |