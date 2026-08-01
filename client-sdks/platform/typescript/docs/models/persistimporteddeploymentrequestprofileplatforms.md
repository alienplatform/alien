# PersistImportedDeploymentRequestProfilePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { PersistImportedDeploymentRequestProfilePlatforms } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestProfilePlatforms = {};
```

## Fields

| Field                                                                                                              | Type                                                                                                               | Required                                                                                                           | Description                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| `aws`                                                                                                              | [models.PersistImportedDeploymentRequestProfileAw](../models/persistimporteddeploymentrequestprofileaw.md)[]       | :heavy_minus_sign:                                                                                                 | AWS permission configurations                                                                                      |
| `azure`                                                                                                            | [models.PersistImportedDeploymentRequestProfileAzure](../models/persistimporteddeploymentrequestprofileazure.md)[] | :heavy_minus_sign:                                                                                                 | Azure permission configurations                                                                                    |
| `gcp`                                                                                                              | [models.PersistImportedDeploymentRequestProfileGcp](../models/persistimporteddeploymentrequestprofilegcp.md)[]     | :heavy_minus_sign:                                                                                                 | GCP permission configurations                                                                                      |