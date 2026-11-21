# TargetDeploymentPermissionSetPlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { TargetDeploymentPermissionSetPlatforms } from "@alienplatform/platform-api/models";

let value: TargetDeploymentPermissionSetPlatforms = {};
```

## Fields

| Field                                                                                    | Type                                                                                     | Required                                                                                 | Description                                                                              |
| ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `aws`                                                                                    | [models.TargetDeploymentPermissionSetAw](../models/targetdeploymentpermissionsetaw.md)[] | :heavy_minus_sign:                                                                       | AWS permission configurations                                                            |
| `azure`                                                                                  | [models.ReleaseInfoPermissionSetAzure](../models/releaseinfopermissionsetazure.md)[]     | :heavy_minus_sign:                                                                       | Azure permission configurations                                                          |
| `gcp`                                                                                    | [models.ReleaseInfoPermissionSetGcp](../models/releaseinfopermissionsetgcp.md)[]         | :heavy_minus_sign:                                                                       | GCP permission configurations                                                            |