# DeploymentExtendAzureBinding

Generic binding configuration for permissions

## Example Usage

```typescript
import { DeploymentExtendAzureBinding } from "@alienplatform/platform-api/models";

let value: DeploymentExtendAzureBinding = {};
```

## Fields

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `resource`                                                                         | [models.DeploymentExtendAzureResource](../models/deploymentextendazureresource.md) | :heavy_minus_sign:                                                                 | Azure-specific binding specification                                               |
| `stack`                                                                            | [models.DeploymentExtendAzureStack](../models/deploymentextendazurestack.md)       | :heavy_minus_sign:                                                                 | Azure-specific binding specification                                               |