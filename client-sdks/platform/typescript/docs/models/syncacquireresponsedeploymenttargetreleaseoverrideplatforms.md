# SyncAcquireResponseDeploymentTargetReleaseOverridePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentTargetReleaseOverridePlatforms } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentTargetReleaseOverridePlatforms = {};
```

## Fields

| Field                                                                                                                                    | Type                                                                                                                                     | Required                                                                                                                                 | Description                                                                                                                              |
| ---------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                                                    | [models.SyncAcquireResponseDeploymentTargetReleaseOverrideAw](../models/syncacquireresponsedeploymenttargetreleaseoverrideaw.md)[]       | :heavy_minus_sign:                                                                                                                       | AWS permission configurations                                                                                                            |
| `azure`                                                                                                                                  | [models.SyncAcquireResponseDeploymentTargetReleaseOverrideAzure](../models/syncacquireresponsedeploymenttargetreleaseoverrideazure.md)[] | :heavy_minus_sign:                                                                                                                       | Azure permission configurations                                                                                                          |
| `gcp`                                                                                                                                    | [models.SyncAcquireResponseDeploymentTargetReleaseOverrideGcp](../models/syncacquireresponsedeploymenttargetreleaseoverridegcp.md)[]     | :heavy_minus_sign:                                                                                                                       | GCP permission configurations                                                                                                            |