# PersistImportedDeploymentRequestExtendAzure

Azure-specific platform permission configuration

## Example Usage

```typescript
import { PersistImportedDeploymentRequestExtendAzure } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestExtendAzure = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                        | Type                                                                                                                         | Required                                                                                                                     | Description                                                                                                                  |
| ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                    | [models.PersistImportedDeploymentRequestExtendAzureBinding](../models/persistimporteddeploymentrequestextendazurebinding.md) | :heavy_check_mark:                                                                                                           | Generic binding configuration for permissions                                                                                |
| `grant`                                                                                                                      | [models.PersistImportedDeploymentRequestExtendAzureGrant](../models/persistimporteddeploymentrequestextendazuregrant.md)     | :heavy_check_mark:                                                                                                           | Grant permissions for a specific cloud platform                                                                              |