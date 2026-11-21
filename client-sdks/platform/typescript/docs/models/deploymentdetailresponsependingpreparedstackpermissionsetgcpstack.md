# DeploymentDetailResponsePendingPreparedStackPermissionSetGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { DeploymentDetailResponsePendingPreparedStackPermissionSetGcpStack } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponsePendingPreparedStackPermissionSetGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                                                    | Type                                                                                     | Required                                                                                 | Description                                                                              |
| ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `condition`                                                                              | *models.DeploymentDetailResponsePendingPreparedStackPermissionSetGcpStackConditionUnion* | :heavy_minus_sign:                                                                       | N/A                                                                                      |
| `scope`                                                                                  | *string*                                                                                 | :heavy_check_mark:                                                                       | Scope (project/resource level)                                                           |