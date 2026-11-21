# DeploymentDetailResponsePreparedStackPermissionSetPlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { DeploymentDetailResponsePreparedStackPermissionSetPlatforms } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponsePreparedStackPermissionSetPlatforms = {};
```

## Fields

| Field                                                                                                                                    | Type                                                                                                                                     | Required                                                                                                                                 | Description                                                                                                                              |
| ---------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                                                    | [models.DeploymentDetailResponsePreparedStackPermissionSetAw](../models/deploymentdetailresponsepreparedstackpermissionsetaw.md)[]       | :heavy_minus_sign:                                                                                                                       | AWS permission configurations                                                                                                            |
| `azure`                                                                                                                                  | [models.DeploymentDetailResponsePreparedStackPermissionSetAzure](../models/deploymentdetailresponsepreparedstackpermissionsetazure.md)[] | :heavy_minus_sign:                                                                                                                       | Azure permission configurations                                                                                                          |
| `gcp`                                                                                                                                    | [models.DeploymentDetailResponsePreparedStackPermissionSetGcp](../models/deploymentdetailresponsepreparedstackpermissionsetgcp.md)[]     | :heavy_minus_sign:                                                                                                                       | GCP permission configurations                                                                                                            |