# DeploymentStatePendingPreparedStackPermissionSetGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { DeploymentStatePendingPreparedStackPermissionSetGcpStack } from "@alienplatform/platform-api/models";

let value: DeploymentStatePendingPreparedStackPermissionSetGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                                           | Type                                                                            | Required                                                                        | Description                                                                     |
| ------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- |
| `condition`                                                                     | *models.DeploymentStatePendingPreparedStackPermissionSetGcpStackConditionUnion* | :heavy_minus_sign:                                                              | N/A                                                                             |
| `scope`                                                                         | *string*                                                                        | :heavy_check_mark:                                                              | Scope (project/resource level)                                                  |