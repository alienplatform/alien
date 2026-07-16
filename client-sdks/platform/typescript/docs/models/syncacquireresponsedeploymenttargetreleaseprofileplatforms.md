# SyncAcquireResponseDeploymentTargetReleaseProfilePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentTargetReleaseProfilePlatforms } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentTargetReleaseProfilePlatforms = {};
```

## Fields

| Field                                                                                                                                  | Type                                                                                                                                   | Required                                                                                                                               | Description                                                                                                                            |
| -------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                                                  | [models.SyncAcquireResponseDeploymentTargetReleaseProfileAw](../models/syncacquireresponsedeploymenttargetreleaseprofileaw.md)[]       | :heavy_minus_sign:                                                                                                                     | AWS permission configurations                                                                                                          |
| `azure`                                                                                                                                | [models.SyncAcquireResponseDeploymentTargetReleaseProfileAzure](../models/syncacquireresponsedeploymenttargetreleaseprofileazure.md)[] | :heavy_minus_sign:                                                                                                                     | Azure permission configurations                                                                                                        |
| `gcp`                                                                                                                                  | [models.SyncAcquireResponseDeploymentTargetReleaseProfileGcp](../models/syncacquireresponsedeploymenttargetreleaseprofilegcp.md)[]     | :heavy_minus_sign:                                                                                                                     | GCP permission configurations                                                                                                          |