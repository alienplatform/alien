# TargetDeploymentExtendGcpBinding

Generic binding configuration for permissions

## Example Usage

```typescript
import { TargetDeploymentExtendGcpBinding } from "@alienplatform/platform-api/models";

let value: TargetDeploymentExtendGcpBinding = {};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `resource`                                                                                 | [models.TargetDeploymentExtendGcpResource](../models/targetdeploymentextendgcpresource.md) | :heavy_minus_sign:                                                                         | GCP-specific binding specification                                                         |
| `stack`                                                                                    | [models.TargetDeploymentExtendGcpStack](../models/targetdeploymentextendgcpstack.md)       | :heavy_minus_sign:                                                                         | GCP-specific binding specification                                                         |