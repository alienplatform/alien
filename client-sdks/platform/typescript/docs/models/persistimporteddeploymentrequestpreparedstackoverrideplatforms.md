# PersistImportedDeploymentRequestPreparedStackOverridePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { PersistImportedDeploymentRequestPreparedStackOverridePlatforms } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestPreparedStackOverridePlatforms = {};
```

## Fields

| Field                                                                                                                                          | Type                                                                                                                                           | Required                                                                                                                                       | Description                                                                                                                                    |
| ---------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                                                          | [models.PersistImportedDeploymentRequestPreparedStackOverrideAw](../models/persistimporteddeploymentrequestpreparedstackoverrideaw.md)[]       | :heavy_minus_sign:                                                                                                                             | AWS permission configurations                                                                                                                  |
| `azure`                                                                                                                                        | [models.PersistImportedDeploymentRequestPreparedStackOverrideAzure](../models/persistimporteddeploymentrequestpreparedstackoverrideazure.md)[] | :heavy_minus_sign:                                                                                                                             | Azure permission configurations                                                                                                                |
| `gcp`                                                                                                                                          | [models.PersistImportedDeploymentRequestPreparedStackOverrideGcp](../models/persistimporteddeploymentrequestpreparedstackoverridegcp.md)[]     | :heavy_minus_sign:                                                                                                                             | GCP permission configurations                                                                                                                  |
