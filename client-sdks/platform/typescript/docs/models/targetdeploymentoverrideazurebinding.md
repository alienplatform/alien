# TargetDeploymentOverrideAzureBinding

Generic binding configuration for permissions

## Example Usage

```typescript
import { TargetDeploymentOverrideAzureBinding } from "@alienplatform/platform-api/models";

let value: TargetDeploymentOverrideAzureBinding = {};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `resource`                                                                                         | [models.TargetDeploymentOverrideAzureResource](../models/targetdeploymentoverrideazureresource.md) | :heavy_minus_sign:                                                                                 | Azure-specific binding specification                                                               |
| `stack`                                                                                            | [models.TargetDeploymentOverrideAzureStack](../models/targetdeploymentoverrideazurestack.md)       | :heavy_minus_sign:                                                                                 | Azure-specific binding specification                                                               |