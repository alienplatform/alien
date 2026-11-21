# DeploymentPendingPreparedStackPermissionSetPlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { DeploymentPendingPreparedStackPermissionSetPlatforms } from "@alienplatform/platform-api/models";

let value: DeploymentPendingPreparedStackPermissionSetPlatforms = {};
```

## Fields

| Field                                                                                                                      | Type                                                                                                                       | Required                                                                                                                   | Description                                                                                                                |
| -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                                      | [models.DeploymentPendingPreparedStackPermissionSetAw](../models/deploymentpendingpreparedstackpermissionsetaw.md)[]       | :heavy_minus_sign:                                                                                                         | AWS permission configurations                                                                                              |
| `azure`                                                                                                                    | [models.DeploymentPendingPreparedStackPermissionSetAzure](../models/deploymentpendingpreparedstackpermissionsetazure.md)[] | :heavy_minus_sign:                                                                                                         | Azure permission configurations                                                                                            |
| `gcp`                                                                                                                      | [models.DeploymentPendingPreparedStackPermissionSetGcp](../models/deploymentpendingpreparedstackpermissionsetgcp.md)[]     | :heavy_minus_sign:                                                                                                         | GCP permission configurations                                                                                              |