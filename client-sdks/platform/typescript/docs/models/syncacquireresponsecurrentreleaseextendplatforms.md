# SyncAcquireResponseCurrentReleaseExtendPlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { SyncAcquireResponseCurrentReleaseExtendPlatforms } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseCurrentReleaseExtendPlatforms = {};
```

## Fields

| Field                                                                                                              | Type                                                                                                               | Required                                                                                                           | Description                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| `aws`                                                                                                              | [models.SyncAcquireResponseCurrentReleaseExtendAw](../models/syncacquireresponsecurrentreleaseextendaw.md)[]       | :heavy_minus_sign:                                                                                                 | AWS permission configurations                                                                                      |
| `azure`                                                                                                            | [models.SyncAcquireResponseCurrentReleaseExtendAzure](../models/syncacquireresponsecurrentreleaseextendazure.md)[] | :heavy_minus_sign:                                                                                                 | Azure permission configurations                                                                                    |
| `gcp`                                                                                                              | [models.SyncAcquireResponseCurrentReleaseExtendGcp](../models/syncacquireresponsecurrentreleaseextendgcp.md)[]     | :heavy_minus_sign:                                                                                                 | GCP permission configurations                                                                                      |