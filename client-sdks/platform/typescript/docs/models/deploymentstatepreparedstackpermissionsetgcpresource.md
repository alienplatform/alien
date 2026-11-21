# DeploymentStatePreparedStackPermissionSetGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { DeploymentStatePreparedStackPermissionSetGcpResource } from "@alienplatform/platform-api/models";

let value: DeploymentStatePreparedStackPermissionSetGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                                       | Type                                                                        | Required                                                                    | Description                                                                 |
| --------------------------------------------------------------------------- | --------------------------------------------------------------------------- | --------------------------------------------------------------------------- | --------------------------------------------------------------------------- |
| `condition`                                                                 | *models.DeploymentStatePreparedStackPermissionSetGcpResourceConditionUnion* | :heavy_minus_sign:                                                          | N/A                                                                         |
| `scope`                                                                     | *string*                                                                    | :heavy_check_mark:                                                          | Scope (project/resource level)                                              |