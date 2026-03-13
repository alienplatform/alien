# DeploymentOverrideGcpBinding

Generic binding configuration for permissions

## Example Usage

```typescript
import { DeploymentOverrideGcpBinding } from "@aliendotdev/platform-api/models";

let value: DeploymentOverrideGcpBinding = {};
```

## Fields

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `resource`                                                                         | [models.DeploymentOverrideGcpResource](../models/deploymentoverridegcpresource.md) | :heavy_minus_sign:                                                                 | GCP-specific binding specification                                                 |
| `stack`                                                                            | [models.DeploymentOverrideGcpStack](../models/deploymentoverridegcpstack.md)       | :heavy_minus_sign:                                                                 | GCP-specific binding specification                                                 |