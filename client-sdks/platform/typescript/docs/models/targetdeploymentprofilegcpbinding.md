# TargetDeploymentProfileGcpBinding

Generic binding configuration for permissions

## Example Usage

```typescript
import { TargetDeploymentProfileGcpBinding } from "@alienplatform/platform-api/models";

let value: TargetDeploymentProfileGcpBinding = {};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `resource`                                                                                   | [models.TargetDeploymentProfileGcpResource](../models/targetdeploymentprofilegcpresource.md) | :heavy_minus_sign:                                                                           | GCP-specific binding specification                                                           |
| `stack`                                                                                      | [models.TargetDeploymentProfileGcpStack](../models/targetdeploymentprofilegcpstack.md)       | :heavy_minus_sign:                                                                           | GCP-specific binding specification                                                           |