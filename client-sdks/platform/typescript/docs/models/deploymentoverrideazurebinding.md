# DeploymentOverrideAzureBinding

Generic binding configuration for permissions

## Example Usage

```typescript
import { DeploymentOverrideAzureBinding } from "@aliendotdev/platform-api/models";

let value: DeploymentOverrideAzureBinding = {};
```

## Fields

| Field                                                                                  | Type                                                                                   | Required                                                                               | Description                                                                            |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `resource`                                                                             | [models.DeploymentOverrideAzureResource](../models/deploymentoverrideazureresource.md) | :heavy_minus_sign:                                                                     | Azure-specific binding specification                                                   |
| `stack`                                                                                | [models.DeploymentOverrideAzureStack](../models/deploymentoverrideazurestack.md)       | :heavy_minus_sign:                                                                     | Azure-specific binding specification                                                   |