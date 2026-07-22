# PersistImportedDeploymentRequestPendingPreparedStackOverridePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { PersistImportedDeploymentRequestPendingPreparedStackOverridePlatforms } from "@alienplatform/platform-api/models";

let value:
  PersistImportedDeploymentRequestPendingPreparedStackOverridePlatforms = {};
```

## Fields

| Field                                                                                                                                                        | Type                                                                                                                                                         | Required                                                                                                                                                     | Description                                                                                                                                                  |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `aws`                                                                                                                                                        | [models.PersistImportedDeploymentRequestPendingPreparedStackOverrideAw](../models/persistimporteddeploymentrequestpendingpreparedstackoverrideaw.md)[]       | :heavy_minus_sign:                                                                                                                                           | AWS permission configurations                                                                                                                                |
| `azure`                                                                                                                                                      | [models.PersistImportedDeploymentRequestPendingPreparedStackOverrideAzure](../models/persistimporteddeploymentrequestpendingpreparedstackoverrideazure.md)[] | :heavy_minus_sign:                                                                                                                                           | Azure permission configurations                                                                                                                              |
| `gcp`                                                                                                                                                        | [models.PersistImportedDeploymentRequestPendingPreparedStackOverrideGcp](../models/persistimporteddeploymentrequestpendingpreparedstackoverridegcp.md)[]     | :heavy_minus_sign:                                                                                                                                           | GCP permission configurations                                                                                                                                |
