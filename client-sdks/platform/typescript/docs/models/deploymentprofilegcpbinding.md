# DeploymentProfileGcpBinding

Generic binding configuration for permissions

## Example Usage

```typescript
import { DeploymentProfileGcpBinding } from "@alienplatform/platform-api/models";

let value: DeploymentProfileGcpBinding = {};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `resource`                                                                       | [models.DeploymentProfileGcpResource](../models/deploymentprofilegcpresource.md) | :heavy_minus_sign:                                                               | GCP-specific binding specification                                               |
| `stack`                                                                          | [models.DeploymentProfileGcpStack](../models/deploymentprofilegcpstack.md)       | :heavy_minus_sign:                                                               | GCP-specific binding specification                                               |