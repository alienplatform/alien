# SyncAcquireResponseDeploymentTargetReleaseExtendPlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentTargetReleaseExtendPlatforms } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentTargetReleaseExtendPlatforms = {};
```

## Fields

| Field                                                                                                                                | Type                                                                                                                                 | Required                                                                                                                             | Description                                                                                                                          |
| ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ |
| `aws`                                                                                                                                | [models.SyncAcquireResponseDeploymentTargetReleaseExtendAw](../models/syncacquireresponsedeploymenttargetreleaseextendaw.md)[]       | :heavy_minus_sign:                                                                                                                   | AWS permission configurations                                                                                                        |
| `azure`                                                                                                                              | [models.SyncAcquireResponseDeploymentTargetReleaseExtendAzure](../models/syncacquireresponsedeploymenttargetreleaseextendazure.md)[] | :heavy_minus_sign:                                                                                                                   | Azure permission configurations                                                                                                      |
| `gcp`                                                                                                                                | [models.SyncAcquireResponseDeploymentTargetReleaseExtendGcp](../models/syncacquireresponsedeploymenttargetreleaseextendgcp.md)[]     | :heavy_minus_sign:                                                                                                                   | GCP permission configurations                                                                                                        |