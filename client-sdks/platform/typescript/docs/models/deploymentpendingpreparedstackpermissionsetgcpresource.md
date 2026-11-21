# DeploymentPendingPreparedStackPermissionSetGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { DeploymentPendingPreparedStackPermissionSetGcpResource } from "@alienplatform/platform-api/models";

let value: DeploymentPendingPreparedStackPermissionSetGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                                         | Type                                                                          | Required                                                                      | Description                                                                   |
| ----------------------------------------------------------------------------- | ----------------------------------------------------------------------------- | ----------------------------------------------------------------------------- | ----------------------------------------------------------------------------- |
| `condition`                                                                   | *models.DeploymentPendingPreparedStackPermissionSetGcpResourceConditionUnion* | :heavy_minus_sign:                                                            | N/A                                                                           |
| `scope`                                                                       | *string*                                                                      | :heavy_check_mark:                                                            | Scope (project/resource level)                                                |