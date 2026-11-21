# PreparedDeploymentStackPermissionSetPlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { PreparedDeploymentStackPermissionSetPlatforms } from "@alienplatform/platform-api/models";

let value: PreparedDeploymentStackPermissionSetPlatforms = {};
```

## Fields

| Field                                                                                                        | Type                                                                                                         | Required                                                                                                     | Description                                                                                                  |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ |
| `aws`                                                                                                        | [models.PreparedDeploymentStackPermissionSetAw](../models/prepareddeploymentstackpermissionsetaw.md)[]       | :heavy_minus_sign:                                                                                           | AWS permission configurations                                                                                |
| `azure`                                                                                                      | [models.PreparedDeploymentStackPermissionSetAzure](../models/prepareddeploymentstackpermissionsetazure.md)[] | :heavy_minus_sign:                                                                                           | Azure permission configurations                                                                              |
| `gcp`                                                                                                        | [models.PreparedDeploymentStackPermissionSetGcp](../models/prepareddeploymentstackpermissionsetgcp.md)[]     | :heavy_minus_sign:                                                                                           | GCP permission configurations                                                                                |