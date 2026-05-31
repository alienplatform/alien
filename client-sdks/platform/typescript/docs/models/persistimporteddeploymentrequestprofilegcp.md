# PersistImportedDeploymentRequestProfileGcp

GCP-specific platform permission configuration

## Example Usage

```typescript
import { PersistImportedDeploymentRequestProfileGcp } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestProfileGcp = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                      | Type                                                                                                                       | Required                                                                                                                   | Description                                                                                                                |
| -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                  | [models.PersistImportedDeploymentRequestProfileGcpBinding](../models/persistimporteddeploymentrequestprofilegcpbinding.md) | :heavy_check_mark:                                                                                                         | Generic binding configuration for permissions                                                                              |
| `description`                                                                                                              | *string*                                                                                                                   | :heavy_minus_sign:                                                                                                         | Short admin-facing description of why this entry exists.                                                                   |
| `grant`                                                                                                                    | [models.PersistImportedDeploymentRequestProfileGcpGrant](../models/persistimporteddeploymentrequestprofilegcpgrant.md)     | :heavy_check_mark:                                                                                                         | Grant permissions for a specific cloud platform                                                                            |
| `label`                                                                                                                    | *string*                                                                                                                   | :heavy_minus_sign:                                                                                                         | Stable admin-facing label for this permission entry.                                                                       |