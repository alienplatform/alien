# SyncAcquireResponseCurrentReleaseExtendGcp

GCP-specific platform permission configuration

## Example Usage

```typescript
import { SyncAcquireResponseCurrentReleaseExtendGcp } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponseCurrentReleaseExtendGcp = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                      | Type                                                                                                                       | Required                                                                                                                   | Description                                                                                                                |
| -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                  | [models.SyncAcquireResponseCurrentReleaseExtendGcpBinding](../models/syncacquireresponsecurrentreleaseextendgcpbinding.md) | :heavy_check_mark:                                                                                                         | Generic binding configuration for permissions                                                                              |
| `grant`                                                                                                                    | [models.SyncAcquireResponseCurrentReleaseExtendGcpGrant](../models/syncacquireresponsecurrentreleaseextendgcpgrant.md)     | :heavy_check_mark:                                                                                                         | Grant permissions for a specific cloud platform                                                                            |