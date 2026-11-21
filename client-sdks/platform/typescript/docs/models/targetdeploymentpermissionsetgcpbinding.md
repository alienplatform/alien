# TargetDeploymentPermissionSetGcpBinding

Generic binding configuration for permissions

## Example Usage

```typescript
import { TargetDeploymentPermissionSetGcpBinding } from "@alienplatform/platform-api/models";

let value: TargetDeploymentPermissionSetGcpBinding = {};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `resource`                                                                                               | [models.TargetDeploymentPermissionSetGcpResource](../models/targetdeploymentpermissionsetgcpresource.md) | :heavy_minus_sign:                                                                                       | GCP-specific binding specification                                                                       |
| `stack`                                                                                                  | [models.TargetDeploymentPermissionSetGcpStack](../models/targetdeploymentpermissionsetgcpstack.md)       | :heavy_minus_sign:                                                                                       | GCP-specific binding specification                                                                       |