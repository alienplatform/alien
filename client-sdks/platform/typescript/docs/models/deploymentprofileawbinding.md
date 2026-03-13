# DeploymentProfileAwBinding

Generic binding configuration for permissions

## Example Usage

```typescript
import { DeploymentProfileAwBinding } from "@aliendotdev/platform-api/models";

let value: DeploymentProfileAwBinding = {};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `resource`                                                                     | [models.DeploymentProfileAwResource](../models/deploymentprofileawresource.md) | :heavy_minus_sign:                                                             | AWS-specific binding specification                                             |
| `stack`                                                                        | [models.DeploymentProfileAwStack](../models/deploymentprofileawstack.md)       | :heavy_minus_sign:                                                             | AWS-specific binding specification                                             |