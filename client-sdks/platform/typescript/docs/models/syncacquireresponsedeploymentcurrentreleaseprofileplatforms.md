# SyncAcquireResponseDeploymentCurrentReleaseProfilePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentCurrentReleaseProfilePlatforms } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentCurrentReleaseProfilePlatforms = {};
```

## Fields

| Field                                                                                                                                    | Type                                                                                                                                     | Required                                                                                                                                 | Description                                                                                                                              |
| ---------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                                                    | [models.SyncAcquireResponseDeploymentCurrentReleaseProfileAw](../models/syncacquireresponsedeploymentcurrentreleaseprofileaw.md)[]       | :heavy_minus_sign:                                                                                                                       | AWS permission configurations                                                                                                            |
| `azure`                                                                                                                                  | [models.SyncAcquireResponseDeploymentCurrentReleaseProfileAzure](../models/syncacquireresponsedeploymentcurrentreleaseprofileazure.md)[] | :heavy_minus_sign:                                                                                                                       | Azure permission configurations                                                                                                          |
| `gcp`                                                                                                                                    | [models.SyncAcquireResponseDeploymentCurrentReleaseProfileGcp](../models/syncacquireresponsedeploymentcurrentreleaseprofilegcp.md)[]     | :heavy_minus_sign:                                                                                                                       | GCP permission configurations                                                                                                            |