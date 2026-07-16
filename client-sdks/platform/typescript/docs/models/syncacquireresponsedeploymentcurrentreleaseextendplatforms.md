# SyncAcquireResponseDeploymentCurrentReleaseExtendPlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentCurrentReleaseExtendPlatforms } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentCurrentReleaseExtendPlatforms = {};
```

## Fields

| Field                                                                                                                                  | Type                                                                                                                                   | Required                                                                                                                               | Description                                                                                                                            |
| -------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                                                  | [models.SyncAcquireResponseDeploymentCurrentReleaseExtendAw](../models/syncacquireresponsedeploymentcurrentreleaseextendaw.md)[]       | :heavy_minus_sign:                                                                                                                     | AWS permission configurations                                                                                                          |
| `azure`                                                                                                                                | [models.SyncAcquireResponseDeploymentCurrentReleaseExtendAzure](../models/syncacquireresponsedeploymentcurrentreleaseextendazure.md)[] | :heavy_minus_sign:                                                                                                                     | Azure permission configurations                                                                                                        |
| `gcp`                                                                                                                                  | [models.SyncAcquireResponseDeploymentCurrentReleaseExtendGcp](../models/syncacquireresponsedeploymentcurrentreleaseextendgcp.md)[]     | :heavy_minus_sign:                                                                                                                     | GCP permission configurations                                                                                                          |