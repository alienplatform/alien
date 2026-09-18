# TargetDeploymentProfileAzureBinding

Generic binding configuration for permissions

## Example Usage

```typescript
import { TargetDeploymentProfileAzureBinding } from "@alienplatform/platform-api/models";

let value: TargetDeploymentProfileAzureBinding = {};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `resource`                                                                                       | [models.TargetDeploymentProfileAzureResource](../models/targetdeploymentprofileazureresource.md) | :heavy_minus_sign:                                                                               | Azure-specific binding specification                                                             |
| `stack`                                                                                          | [models.TargetDeploymentProfileAzureStack](../models/targetdeploymentprofileazurestack.md)       | :heavy_minus_sign:                                                                               | Azure-specific binding specification                                                             |