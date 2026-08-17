# TargetDeploymentExtendAzureBinding

Generic binding configuration for permissions

## Example Usage

```typescript
import { TargetDeploymentExtendAzureBinding } from "@alienplatform/platform-api/models";

let value: TargetDeploymentExtendAzureBinding = {};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `resource`                                                                                     | [models.TargetDeploymentExtendAzureResource](../models/targetdeploymentextendazureresource.md) | :heavy_minus_sign:                                                                             | Azure-specific binding specification                                                           |
| `stack`                                                                                        | [models.TargetDeploymentExtendAzureStack](../models/targetdeploymentextendazurestack.md)       | :heavy_minus_sign:                                                                             | Azure-specific binding specification                                                           |