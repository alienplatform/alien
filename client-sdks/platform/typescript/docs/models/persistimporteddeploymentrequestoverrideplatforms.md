# PersistImportedDeploymentRequestOverridePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { PersistImportedDeploymentRequestOverridePlatforms } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestOverridePlatforms = {};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                                | [models.PersistImportedDeploymentRequestOverrideAw](../models/persistimporteddeploymentrequestoverrideaw.md)[]       | :heavy_minus_sign:                                                                                                   | AWS permission configurations                                                                                        |
| `azure`                                                                                                              | [models.PersistImportedDeploymentRequestOverrideAzure](../models/persistimporteddeploymentrequestoverrideazure.md)[] | :heavy_minus_sign:                                                                                                   | Azure permission configurations                                                                                      |
| `gcp`                                                                                                                | [models.PersistImportedDeploymentRequestOverrideGcp](../models/persistimporteddeploymentrequestoverridegcp.md)[]     | :heavy_minus_sign:                                                                                                   | GCP permission configurations                                                                                        |