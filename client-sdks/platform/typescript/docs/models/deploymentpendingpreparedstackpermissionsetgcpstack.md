# DeploymentPendingPreparedStackPermissionSetGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { DeploymentPendingPreparedStackPermissionSetGcpStack } from "@alienplatform/platform-api/models";

let value: DeploymentPendingPreparedStackPermissionSetGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `condition`                                                                | *models.DeploymentPendingPreparedStackPermissionSetGcpStackConditionUnion* | :heavy_minus_sign:                                                         | N/A                                                                        |
| `scope`                                                                    | *string*                                                                   | :heavy_check_mark:                                                         | Scope (project/resource level)                                             |