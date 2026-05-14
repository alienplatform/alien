# PersistImportedDeploymentRequestExtendPlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { PersistImportedDeploymentRequestExtendPlatforms } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestExtendPlatforms = {};
```

## Fields

| Field                                                                                                            | Type                                                                                                             | Required                                                                                                         | Description                                                                                                      |
| ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                            | [models.PersistImportedDeploymentRequestExtendAw](../models/persistimporteddeploymentrequestextendaw.md)[]       | :heavy_minus_sign:                                                                                               | AWS permission configurations                                                                                    |
| `azure`                                                                                                          | [models.PersistImportedDeploymentRequestExtendAzure](../models/persistimporteddeploymentrequestextendazure.md)[] | :heavy_minus_sign:                                                                                               | Azure permission configurations                                                                                  |
| `gcp`                                                                                                            | [models.PersistImportedDeploymentRequestExtendGcp](../models/persistimporteddeploymentrequestextendgcp.md)[]     | :heavy_minus_sign:                                                                                               | GCP permission configurations                                                                                    |