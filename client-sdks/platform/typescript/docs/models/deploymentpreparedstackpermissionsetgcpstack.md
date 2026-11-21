# DeploymentPreparedStackPermissionSetGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { DeploymentPreparedStackPermissionSetGcpStack } from "@alienplatform/platform-api/models";

let value: DeploymentPreparedStackPermissionSetGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                               | Type                                                                | Required                                                            | Description                                                         |
| ------------------------------------------------------------------- | ------------------------------------------------------------------- | ------------------------------------------------------------------- | ------------------------------------------------------------------- |
| `condition`                                                         | *models.DeploymentPreparedStackPermissionSetGcpStackConditionUnion* | :heavy_minus_sign:                                                  | N/A                                                                 |
| `scope`                                                             | *string*                                                            | :heavy_check_mark:                                                  | Scope (project/resource level)                                      |