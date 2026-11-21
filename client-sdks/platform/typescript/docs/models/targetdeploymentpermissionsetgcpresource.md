# TargetDeploymentPermissionSetGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { TargetDeploymentPermissionSetGcpResource } from "@alienplatform/platform-api/models";

let value: TargetDeploymentPermissionSetGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                           | Type                                                            | Required                                                        | Description                                                     |
| --------------------------------------------------------------- | --------------------------------------------------------------- | --------------------------------------------------------------- | --------------------------------------------------------------- |
| `condition`                                                     | *models.TargetDeploymentPermissionSetGcpResourceConditionUnion* | :heavy_minus_sign:                                              | N/A                                                             |
| `scope`                                                         | *string*                                                        | :heavy_check_mark:                                              | Scope (project/resource level)                                  |