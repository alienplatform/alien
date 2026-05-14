# PersistImportedDeploymentRequestOverrideAzure

Azure-specific platform permission configuration

## Example Usage

```typescript
import { PersistImportedDeploymentRequestOverrideAzure } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestOverrideAzure = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                            | Type                                                                                                                             | Required                                                                                                                         | Description                                                                                                                      |
| -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                        | [models.PersistImportedDeploymentRequestOverrideAzureBinding](../models/persistimporteddeploymentrequestoverrideazurebinding.md) | :heavy_check_mark:                                                                                                               | Generic binding configuration for permissions                                                                                    |
| `grant`                                                                                                                          | [models.PersistImportedDeploymentRequestOverrideAzureGrant](../models/persistimporteddeploymentrequestoverrideazuregrant.md)     | :heavy_check_mark:                                                                                                               | Grant permissions for a specific cloud platform                                                                                  |