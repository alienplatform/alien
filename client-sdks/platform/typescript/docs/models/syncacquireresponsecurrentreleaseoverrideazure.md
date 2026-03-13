# SyncAcquireResponseCurrentReleaseOverrideAzure

Azure-specific platform permission configuration

## Example Usage

```typescript
import { SyncAcquireResponseCurrentReleaseOverrideAzure } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponseCurrentReleaseOverrideAzure = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                              | Type                                                                                                                               | Required                                                                                                                           | Description                                                                                                                        |
| ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                          | [models.SyncAcquireResponseCurrentReleaseOverrideAzureBinding](../models/syncacquireresponsecurrentreleaseoverrideazurebinding.md) | :heavy_check_mark:                                                                                                                 | Generic binding configuration for permissions                                                                                      |
| `grant`                                                                                                                            | [models.SyncAcquireResponseCurrentReleaseOverrideAzureGrant](../models/syncacquireresponsecurrentreleaseoverrideazuregrant.md)     | :heavy_check_mark:                                                                                                                 | Grant permissions for a specific cloud platform                                                                                    |