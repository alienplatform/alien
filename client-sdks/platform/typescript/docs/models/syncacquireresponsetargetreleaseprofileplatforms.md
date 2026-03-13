# SyncAcquireResponseTargetReleaseProfilePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { SyncAcquireResponseTargetReleaseProfilePlatforms } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponseTargetReleaseProfilePlatforms = {};
```

## Fields

| Field                                                                                                              | Type                                                                                                               | Required                                                                                                           | Description                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| `aws`                                                                                                              | [models.SyncAcquireResponseTargetReleaseProfileAw](../models/syncacquireresponsetargetreleaseprofileaw.md)[]       | :heavy_minus_sign:                                                                                                 | AWS permission configurations                                                                                      |
| `azure`                                                                                                            | [models.SyncAcquireResponseTargetReleaseProfileAzure](../models/syncacquireresponsetargetreleaseprofileazure.md)[] | :heavy_minus_sign:                                                                                                 | Azure permission configurations                                                                                    |
| `gcp`                                                                                                              | [models.SyncAcquireResponseTargetReleaseProfileGcp](../models/syncacquireresponsetargetreleaseprofilegcp.md)[]     | :heavy_minus_sign:                                                                                                 | GCP permission configurations                                                                                      |