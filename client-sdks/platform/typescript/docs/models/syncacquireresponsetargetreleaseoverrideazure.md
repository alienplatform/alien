# SyncAcquireResponseTargetReleaseOverrideAzure

Azure-specific platform permission configuration

## Example Usage

```typescript
import { SyncAcquireResponseTargetReleaseOverrideAzure } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponseTargetReleaseOverrideAzure = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                            | Type                                                                                                                             | Required                                                                                                                         | Description                                                                                                                      |
| -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                        | [models.SyncAcquireResponseTargetReleaseOverrideAzureBinding](../models/syncacquireresponsetargetreleaseoverrideazurebinding.md) | :heavy_check_mark:                                                                                                               | Generic binding configuration for permissions                                                                                    |
| `grant`                                                                                                                          | [models.SyncAcquireResponseTargetReleaseOverrideAzureGrant](../models/syncacquireresponsetargetreleaseoverrideazuregrant.md)     | :heavy_check_mark:                                                                                                               | Grant permissions for a specific cloud platform                                                                                  |