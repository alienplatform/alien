# DeploymentExtendGcpBinding

Generic binding configuration for permissions

## Example Usage

```typescript
import { DeploymentExtendGcpBinding } from "@alienplatform/platform-api/models";

let value: DeploymentExtendGcpBinding = {};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `resource`                                                                     | [models.DeploymentExtendGcpResource](../models/deploymentextendgcpresource.md) | :heavy_minus_sign:                                                             | GCP-specific binding specification                                             |
| `stack`                                                                        | [models.DeploymentExtendGcpStack](../models/deploymentextendgcpstack.md)       | :heavy_minus_sign:                                                             | GCP-specific binding specification                                             |