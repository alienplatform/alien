# DeploymentOverrideAwBinding

Generic binding configuration for permissions

## Example Usage

```typescript
import { DeploymentOverrideAwBinding } from "@aliendotdev/platform-api/models";

let value: DeploymentOverrideAwBinding = {};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `resource`                                                                       | [models.DeploymentOverrideAwResource](../models/deploymentoverrideawresource.md) | :heavy_minus_sign:                                                               | AWS-specific binding specification                                               |
| `stack`                                                                          | [models.DeploymentOverrideAwStack](../models/deploymentoverrideawstack.md)       | :heavy_minus_sign:                                                               | AWS-specific binding specification                                               |