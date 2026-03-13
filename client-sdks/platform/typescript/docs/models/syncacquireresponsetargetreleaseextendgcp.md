# SyncAcquireResponseTargetReleaseExtendGcp

GCP-specific platform permission configuration

## Example Usage

```typescript
import { SyncAcquireResponseTargetReleaseExtendGcp } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponseTargetReleaseExtendGcp = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                    | Type                                                                                                                     | Required                                                                                                                 | Description                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| `binding`                                                                                                                | [models.SyncAcquireResponseTargetReleaseExtendGcpBinding](../models/syncacquireresponsetargetreleaseextendgcpbinding.md) | :heavy_check_mark:                                                                                                       | Generic binding configuration for permissions                                                                            |
| `grant`                                                                                                                  | [models.SyncAcquireResponseTargetReleaseExtendGcpGrant](../models/syncacquireresponsetargetreleaseextendgcpgrant.md)     | :heavy_check_mark:                                                                                                       | Grant permissions for a specific cloud platform                                                                          |