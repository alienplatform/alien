# PersistImportedDeploymentRequestOverrideGcp

GCP-specific platform permission configuration

## Example Usage

```typescript
import { PersistImportedDeploymentRequestOverrideGcp } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestOverrideGcp = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                        | Type                                                                                                                         | Required                                                                                                                     | Description                                                                                                                  |
| ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                    | [models.PersistImportedDeploymentRequestOverrideGcpBinding](../models/persistimporteddeploymentrequestoverridegcpbinding.md) | :heavy_check_mark:                                                                                                           | Generic binding configuration for permissions                                                                                |
| `grant`                                                                                                                      | [models.PersistImportedDeploymentRequestOverrideGcpGrant](../models/persistimporteddeploymentrequestoverridegcpgrant.md)     | :heavy_check_mark:                                                                                                           | Grant permissions for a specific cloud platform                                                                              |