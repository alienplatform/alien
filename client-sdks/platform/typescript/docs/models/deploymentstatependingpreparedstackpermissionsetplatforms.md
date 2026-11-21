# DeploymentStatePendingPreparedStackPermissionSetPlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { DeploymentStatePendingPreparedStackPermissionSetPlatforms } from "@alienplatform/platform-api/models";

let value: DeploymentStatePendingPreparedStackPermissionSetPlatforms = {};
```

## Fields

| Field                                                                                                                                | Type                                                                                                                                 | Required                                                                                                                             | Description                                                                                                                          |
| ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ |
| `aws`                                                                                                                                | [models.DeploymentStatePendingPreparedStackPermissionSetAw](../models/deploymentstatependingpreparedstackpermissionsetaw.md)[]       | :heavy_minus_sign:                                                                                                                   | AWS permission configurations                                                                                                        |
| `azure`                                                                                                                              | [models.DeploymentStatePendingPreparedStackPermissionSetAzure](../models/deploymentstatependingpreparedstackpermissionsetazure.md)[] | :heavy_minus_sign:                                                                                                                   | Azure permission configurations                                                                                                      |
| `gcp`                                                                                                                                | [models.DeploymentStatePendingPreparedStackPermissionSetGcp](../models/deploymentstatependingpreparedstackpermissionsetgcp.md)[]     | :heavy_minus_sign:                                                                                                                   | GCP permission configurations                                                                                                        |