# DeploymentProfileAzureBinding

Generic binding configuration for permissions

## Example Usage

```typescript
import { DeploymentProfileAzureBinding } from "@alienplatform/platform-api/models";

let value: DeploymentProfileAzureBinding = {};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `resource`                                                                           | [models.DeploymentProfileAzureResource](../models/deploymentprofileazureresource.md) | :heavy_minus_sign:                                                                   | Azure-specific binding specification                                                 |
| `stack`                                                                              | [models.DeploymentProfileAzureStack](../models/deploymentprofileazurestack.md)       | :heavy_minus_sign:                                                                   | Azure-specific binding specification                                                 |