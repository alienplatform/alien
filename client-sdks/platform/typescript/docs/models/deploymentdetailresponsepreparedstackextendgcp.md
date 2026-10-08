# DeploymentDetailResponsePreparedStackExtendGcp

GCP-specific platform permission configuration

## Example Usage

```typescript
import { DeploymentDetailResponsePreparedStackExtendGcp } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponsePreparedStackExtendGcp = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                              | Type                                                                                                                               | Required                                                                                                                           | Description                                                                                                                        |
| ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                          | [models.DeploymentDetailResponsePreparedStackExtendGcpBinding](../models/deploymentdetailresponsepreparedstackextendgcpbinding.md) | :heavy_check_mark:                                                                                                                 | Generic binding configuration for permissions                                                                                      |
| `description`                                                                                                                      | *string*                                                                                                                           | :heavy_minus_sign:                                                                                                                 | Short admin-facing description of why this entry exists.                                                                           |
| `grant`                                                                                                                            | [models.DeploymentDetailResponsePreparedStackExtendGcpGrant](../models/deploymentdetailresponsepreparedstackextendgcpgrant.md)     | :heavy_check_mark:                                                                                                                 | Grant permissions for a specific cloud platform                                                                                    |
| `label`                                                                                                                            | *string*                                                                                                                           | :heavy_minus_sign:                                                                                                                 | Stable admin-facing label for this permission entry.                                                                               |
