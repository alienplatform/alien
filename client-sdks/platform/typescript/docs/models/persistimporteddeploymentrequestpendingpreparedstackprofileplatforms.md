# PersistImportedDeploymentRequestPendingPreparedStackProfilePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { PersistImportedDeploymentRequestPendingPreparedStackProfilePlatforms } from "@alienplatform/platform-api/models";

let value:
  PersistImportedDeploymentRequestPendingPreparedStackProfilePlatforms = {};
```

## Fields

| Field                                                                                                                                                      | Type                                                                                                                                                       | Required                                                                                                                                                   | Description                                                                                                                                                |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                                                                      | [models.PersistImportedDeploymentRequestPendingPreparedStackProfileAw](../models/persistimporteddeploymentrequestpendingpreparedstackprofileaw.md)[]       | :heavy_minus_sign:                                                                                                                                         | AWS permission configurations                                                                                                                              |
| `azure`                                                                                                                                                    | [models.PersistImportedDeploymentRequestPendingPreparedStackProfileAzure](../models/persistimporteddeploymentrequestpendingpreparedstackprofileazure.md)[] | :heavy_minus_sign:                                                                                                                                         | Azure permission configurations                                                                                                                            |
| `gcp`                                                                                                                                                      | [models.PersistImportedDeploymentRequestPendingPreparedStackProfileGcp](../models/persistimporteddeploymentrequestpendingpreparedstackprofilegcp.md)[]     | :heavy_minus_sign:                                                                                                                                         | GCP permission configurations                                                                                                                              |
