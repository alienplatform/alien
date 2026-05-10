# PersistImportedDeploymentRequestProfileAzure

Azure-specific platform permission configuration

## Example Usage

```typescript
import { PersistImportedDeploymentRequestProfileAzure } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestProfileAzure = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                          | Type                                                                                                                           | Required                                                                                                                       | Description                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ |
| `binding`                                                                                                                      | [models.PersistImportedDeploymentRequestProfileAzureBinding](../models/persistimporteddeploymentrequestprofileazurebinding.md) | :heavy_check_mark:                                                                                                             | Generic binding configuration for permissions                                                                                  |
| `grant`                                                                                                                        | [models.PersistImportedDeploymentRequestProfileAzureGrant](../models/persistimporteddeploymentrequestprofileazuregrant.md)     | :heavy_check_mark:                                                                                                             | Grant permissions for a specific cloud platform                                                                                |