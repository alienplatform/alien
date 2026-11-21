# DeploymentStatePreparedStackPermissionSetPlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { DeploymentStatePreparedStackPermissionSetPlatforms } from "@alienplatform/platform-api/models";

let value: DeploymentStatePreparedStackPermissionSetPlatforms = {};
```

## Fields

| Field                                                                                                                  | Type                                                                                                                   | Required                                                                                                               | Description                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                                  | [models.DeploymentStatePreparedStackPermissionSetAw](../models/deploymentstatepreparedstackpermissionsetaw.md)[]       | :heavy_minus_sign:                                                                                                     | AWS permission configurations                                                                                          |
| `azure`                                                                                                                | [models.DeploymentStatePreparedStackPermissionSetAzure](../models/deploymentstatepreparedstackpermissionsetazure.md)[] | :heavy_minus_sign:                                                                                                     | Azure permission configurations                                                                                        |
| `gcp`                                                                                                                  | [models.DeploymentStatePreparedStackPermissionSetGcp](../models/deploymentstatepreparedstackpermissionsetgcp.md)[]     | :heavy_minus_sign:                                                                                                     | GCP permission configurations                                                                                          |