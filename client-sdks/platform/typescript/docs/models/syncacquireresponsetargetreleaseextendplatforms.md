# SyncAcquireResponseTargetReleaseExtendPlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { SyncAcquireResponseTargetReleaseExtendPlatforms } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseTargetReleaseExtendPlatforms = {};
```

## Fields

| Field                                                                                                            | Type                                                                                                             | Required                                                                                                         | Description                                                                                                      |
| ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                            | [models.SyncAcquireResponseTargetReleaseExtendAw](../models/syncacquireresponsetargetreleaseextendaw.md)[]       | :heavy_minus_sign:                                                                                               | AWS permission configurations                                                                                    |
| `azure`                                                                                                          | [models.SyncAcquireResponseTargetReleaseExtendAzure](../models/syncacquireresponsetargetreleaseextendazure.md)[] | :heavy_minus_sign:                                                                                               | Azure permission configurations                                                                                  |
| `gcp`                                                                                                            | [models.SyncAcquireResponseTargetReleaseExtendGcp](../models/syncacquireresponsetargetreleaseextendgcp.md)[]     | :heavy_minus_sign:                                                                                               | GCP permission configurations                                                                                    |