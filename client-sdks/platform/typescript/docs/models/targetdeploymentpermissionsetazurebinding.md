# TargetDeploymentPermissionSetAzureBinding

Generic binding configuration for permissions

## Example Usage

```typescript
import { TargetDeploymentPermissionSetAzureBinding } from "@alienplatform/platform-api/models";

let value: TargetDeploymentPermissionSetAzureBinding = {};
```

## Fields

| Field                                                                                                        | Type                                                                                                         | Required                                                                                                     | Description                                                                                                  |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ |
| `resource`                                                                                                   | [models.TargetDeploymentPermissionSetAzureResource](../models/targetdeploymentpermissionsetazureresource.md) | :heavy_minus_sign:                                                                                           | Azure-specific binding specification                                                                         |
| `stack`                                                                                                      | [models.TargetDeploymentPermissionSetAzureStack](../models/targetdeploymentpermissionsetazurestack.md)       | :heavy_minus_sign:                                                                                           | Azure-specific binding specification                                                                         |