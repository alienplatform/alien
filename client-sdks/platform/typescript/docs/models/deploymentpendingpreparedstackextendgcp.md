# DeploymentPendingPreparedStackExtendGcp

GCP-specific platform permission configuration

## Example Usage

```typescript
import { DeploymentPendingPreparedStackExtendGcp } from "@alienplatform/platform-api/models";

let value: DeploymentPendingPreparedStackExtendGcp = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                            | [models.DeploymentPendingPreparedStackExtendGcpBinding](../models/deploymentpendingpreparedstackextendgcpbinding.md) | :heavy_check_mark:                                                                                                   | Generic binding configuration for permissions                                                                        |
| `description`                                                                                                        | *string*                                                                                                             | :heavy_minus_sign:                                                                                                   | Short admin-facing description of why this entry exists.                                                             |
| `grant`                                                                                                              | [models.DeploymentPendingPreparedStackExtendGcpGrant](../models/deploymentpendingpreparedstackextendgcpgrant.md)     | :heavy_check_mark:                                                                                                   | Grant permissions for a specific cloud platform                                                                      |
| `label`                                                                                                              | *string*                                                                                                             | :heavy_minus_sign:                                                                                                   | Stable admin-facing label for this permission entry.                                                                 |
