# DeploymentDetailResponsePendingPreparedStackPermissionSetPlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { DeploymentDetailResponsePendingPreparedStackPermissionSetPlatforms } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponsePendingPreparedStackPermissionSetPlatforms =
  {};
```

## Fields

| Field                                                                                                                                                  | Type                                                                                                                                                   | Required                                                                                                                                               | Description                                                                                                                                            |
| ------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `aws`                                                                                                                                                  | [models.DeploymentDetailResponsePendingPreparedStackPermissionSetAw](../models/deploymentdetailresponsependingpreparedstackpermissionsetaw.md)[]       | :heavy_minus_sign:                                                                                                                                     | AWS permission configurations                                                                                                                          |
| `azure`                                                                                                                                                | [models.DeploymentDetailResponsePendingPreparedStackPermissionSetAzure](../models/deploymentdetailresponsependingpreparedstackpermissionsetazure.md)[] | :heavy_minus_sign:                                                                                                                                     | Azure permission configurations                                                                                                                        |
| `gcp`                                                                                                                                                  | [models.DeploymentDetailResponsePendingPreparedStackPermissionSetGcp](../models/deploymentdetailresponsependingpreparedstackpermissionsetgcp.md)[]     | :heavy_minus_sign:                                                                                                                                     | GCP permission configurations                                                                                                                          |