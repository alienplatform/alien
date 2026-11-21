# DeploymentStatePendingPreparedStackPermissionSetGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { DeploymentStatePendingPreparedStackPermissionSetGcpResource } from "@alienplatform/platform-api/models";

let value: DeploymentStatePendingPreparedStackPermissionSetGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `condition`                                                                        | *models.DeploymentStatePendingPreparedStackPermissionSetGcpResourceConditionUnion* | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `scope`                                                                            | *string*                                                                           | :heavy_check_mark:                                                                 | Scope (project/resource level)                                                     |