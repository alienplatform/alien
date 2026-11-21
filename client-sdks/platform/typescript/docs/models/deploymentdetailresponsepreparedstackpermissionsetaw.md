# DeploymentDetailResponsePreparedStackPermissionSetAw

AWS-specific platform permission configuration

## Example Usage

```typescript
import { DeploymentDetailResponsePreparedStackPermissionSetAw } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponsePreparedStackPermissionSetAw = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                                          | Type                                                                                                                                           | Required                                                                                                                                       | Description                                                                                                                                    |
| ---------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                                      | [models.DeploymentDetailResponsePreparedStackPermissionSetAwBinding](../models/deploymentdetailresponsepreparedstackpermissionsetawbinding.md) | :heavy_check_mark:                                                                                                                             | Generic binding configuration for permissions                                                                                                  |
| `description`                                                                                                                                  | *string*                                                                                                                                       | :heavy_minus_sign:                                                                                                                             | Short admin-facing description of why this entry exists.                                                                                       |
| `effect`                                                                                                                                       | [models.DeploymentDetailResponsePreparedStackPermissionSetEffect](../models/deploymentdetailresponsepreparedstackpermissionseteffect.md)       | :heavy_minus_sign:                                                                                                                             | IAM effect. Defaults to Allow.                                                                                                                 |
| `grant`                                                                                                                                        | [models.DeploymentDetailResponsePreparedStackPermissionSetAwGrant](../models/deploymentdetailresponsepreparedstackpermissionsetawgrant.md)     | :heavy_check_mark:                                                                                                                             | Grant permissions for a specific cloud platform                                                                                                |
| `label`                                                                                                                                        | *string*                                                                                                                                       | :heavy_minus_sign:                                                                                                                             | Stable admin-facing label for this permission entry.                                                                                           |