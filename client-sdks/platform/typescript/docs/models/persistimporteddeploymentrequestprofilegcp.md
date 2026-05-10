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
| `grant`                                                                                                                    | [models.PersistImportedDeploymentRequestProfileGcpGrant](../models/persistimporteddeploymentrequestprofilegcpgrant.md)     | :heavy_check_mark:                                                                                                         | Grant permissions for a specific cloud platform                                                                            |