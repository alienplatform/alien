# SyncAcquireResponseDeploymentCurrentReleaseOverridePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentCurrentReleaseOverridePlatforms } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentCurrentReleaseOverridePlatforms = {};
```

## Fields

| Field                                                                                                                                      | Type                                                                                                                                       | Required                                                                                                                                   | Description                                                                                                                                |
| ------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------ |
| `aws`                                                                                                                                      | [models.SyncAcquireResponseDeploymentCurrentReleaseOverrideAw](../models/syncacquireresponsedeploymentcurrentreleaseoverrideaw.md)[]       | :heavy_minus_sign:                                                                                                                         | AWS permission configurations                                                                                                              |
| `azure`                                                                                                                                    | [models.SyncAcquireResponseDeploymentCurrentReleaseOverrideAzure](../models/syncacquireresponsedeploymentcurrentreleaseoverrideazure.md)[] | :heavy_minus_sign:                                                                                                                         | Azure permission configurations                                                                                                            |
| `gcp`                                                                                                                                      | [models.SyncAcquireResponseDeploymentCurrentReleaseOverrideGcp](../models/syncacquireresponsedeploymentcurrentreleaseoverridegcp.md)[]     | :heavy_minus_sign:                                                                                                                         | GCP permission configurations                                                                                                              |