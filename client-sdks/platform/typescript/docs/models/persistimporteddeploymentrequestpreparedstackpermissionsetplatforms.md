# PersistImportedDeploymentRequestPreparedStackPermissionSetPlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { PersistImportedDeploymentRequestPreparedStackPermissionSetPlatforms } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestPreparedStackPermissionSetPlatforms =
  {};
```

## Fields

| Field                                                                                                                                                    | Type                                                                                                                                                     | Required                                                                                                                                                 | Description                                                                                                                                              |
| -------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                                                                    | [models.PersistImportedDeploymentRequestPreparedStackPermissionSetAw](../models/persistimporteddeploymentrequestpreparedstackpermissionsetaw.md)[]       | :heavy_minus_sign:                                                                                                                                       | AWS permission configurations                                                                                                                            |
| `azure`                                                                                                                                                  | [models.PersistImportedDeploymentRequestPreparedStackPermissionSetAzure](../models/persistimporteddeploymentrequestpreparedstackpermissionsetazure.md)[] | :heavy_minus_sign:                                                                                                                                       | Azure permission configurations                                                                                                                          |
| `gcp`                                                                                                                                                    | [models.PersistImportedDeploymentRequestPreparedStackPermissionSetGcp](../models/persistimporteddeploymentrequestpreparedstackpermissionsetgcp.md)[]     | :heavy_minus_sign:                                                                                                                                       | GCP permission configurations                                                                                                                            |