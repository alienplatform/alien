# TargetDeploymentOverrideGcpBinding

Generic binding configuration for permissions

## Example Usage

```typescript
import { TargetDeploymentOverrideGcpBinding } from "@alienplatform/platform-api/models";

let value: TargetDeploymentOverrideGcpBinding = {};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `resource`                                                                                     | [models.TargetDeploymentOverrideGcpResource](../models/targetdeploymentoverridegcpresource.md) | :heavy_minus_sign:                                                                             | GCP-specific binding specification                                                             |
| `stack`                                                                                        | [models.TargetDeploymentOverrideGcpStack](../models/targetdeploymentoverridegcpstack.md)       | :heavy_minus_sign:                                                                             | GCP-specific binding specification                                                             |