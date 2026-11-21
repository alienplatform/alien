# PersistImportedDeploymentRequestPendingPreparedStackPermissionSetPlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { PersistImportedDeploymentRequestPendingPreparedStackPermissionSetPlatforms } from "@alienplatform/platform-api/models";

let value:
  PersistImportedDeploymentRequestPendingPreparedStackPermissionSetPlatforms =
    {};
```

## Fields

| Field                                                                                                                                                                  | Type                                                                                                                                                                   | Required                                                                                                                                                               | Description                                                                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                                                                                  | [models.PersistImportedDeploymentRequestPendingPreparedStackPermissionSetAw](../models/persistimporteddeploymentrequestpendingpreparedstackpermissionsetaw.md)[]       | :heavy_minus_sign:                                                                                                                                                     | AWS permission configurations                                                                                                                                          |
| `azure`                                                                                                                                                                | [models.PersistImportedDeploymentRequestPendingPreparedStackPermissionSetAzure](../models/persistimporteddeploymentrequestpendingpreparedstackpermissionsetazure.md)[] | :heavy_minus_sign:                                                                                                                                                     | Azure permission configurations                                                                                                                                        |
| `gcp`                                                                                                                                                                  | [models.PersistImportedDeploymentRequestPendingPreparedStackPermissionSetGcp](../models/persistimporteddeploymentrequestpendingpreparedstackpermissionsetgcp.md)[]     | :heavy_minus_sign:                                                                                                                                                     | GCP permission configurations                                                                                                                                          |