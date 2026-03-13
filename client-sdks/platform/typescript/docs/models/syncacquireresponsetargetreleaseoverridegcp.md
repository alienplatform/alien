# SyncAcquireResponseTargetReleaseOverrideGcp

GCP-specific platform permission configuration

## Example Usage

```typescript
import { SyncAcquireResponseTargetReleaseOverrideGcp } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseTargetReleaseOverrideGcp = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                        | Type                                                                                                                         | Required                                                                                                                     | Description                                                                                                                  |
| ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                    | [models.SyncAcquireResponseTargetReleaseOverrideGcpBinding](../models/syncacquireresponsetargetreleaseoverridegcpbinding.md) | :heavy_check_mark:                                                                                                           | Generic binding configuration for permissions                                                                                |
| `grant`                                                                                                                      | [models.SyncAcquireResponseTargetReleaseOverrideGcpGrant](../models/syncacquireresponsetargetreleaseoverridegcpgrant.md)     | :heavy_check_mark:                                                                                                           | Grant permissions for a specific cloud platform                                                                              |