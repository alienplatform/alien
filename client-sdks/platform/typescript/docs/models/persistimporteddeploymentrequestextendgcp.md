# PersistImportedDeploymentRequestExtendGcp

GCP-specific platform permission configuration

## Example Usage

```typescript
import { PersistImportedDeploymentRequestExtendGcp } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestExtendGcp = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                    | Type                                                                                                                     | Required                                                                                                                 | Description                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| `binding`                                                                                                                | [models.PersistImportedDeploymentRequestExtendGcpBinding](../models/persistimporteddeploymentrequestextendgcpbinding.md) | :heavy_check_mark:                                                                                                       | Generic binding configuration for permissions                                                                            |
| `grant`                                                                                                                  | [models.PersistImportedDeploymentRequestExtendGcpGrant](../models/persistimporteddeploymentrequestextendgcpgrant.md)     | :heavy_check_mark:                                                                                                       | Grant permissions for a specific cloud platform                                                                          |