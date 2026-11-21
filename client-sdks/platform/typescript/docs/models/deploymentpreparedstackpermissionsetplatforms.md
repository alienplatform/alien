# DeploymentPreparedStackPermissionSetPlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { DeploymentPreparedStackPermissionSetPlatforms } from "@alienplatform/platform-api/models";

let value: DeploymentPreparedStackPermissionSetPlatforms = {};
```

## Fields

| Field                                                                                                        | Type                                                                                                         | Required                                                                                                     | Description                                                                                                  |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ |
| `aws`                                                                                                        | [models.DeploymentPreparedStackPermissionSetAw](../models/deploymentpreparedstackpermissionsetaw.md)[]       | :heavy_minus_sign:                                                                                           | AWS permission configurations                                                                                |
| `azure`                                                                                                      | [models.DeploymentPreparedStackPermissionSetAzure](../models/deploymentpreparedstackpermissionsetazure.md)[] | :heavy_minus_sign:                                                                                           | Azure permission configurations                                                                              |
| `gcp`                                                                                                        | [models.DeploymentPreparedStackPermissionSetGcp](../models/deploymentpreparedstackpermissionsetgcp.md)[]     | :heavy_minus_sign:                                                                                           | GCP permission configurations                                                                                |