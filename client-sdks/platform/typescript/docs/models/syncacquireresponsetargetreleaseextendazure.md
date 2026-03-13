# SyncAcquireResponseTargetReleaseExtendAzure

Azure-specific platform permission configuration

## Example Usage

```typescript
import { SyncAcquireResponseTargetReleaseExtendAzure } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseTargetReleaseExtendAzure = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                        | Type                                                                                                                         | Required                                                                                                                     | Description                                                                                                                  |
| ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                    | [models.SyncAcquireResponseTargetReleaseExtendAzureBinding](../models/syncacquireresponsetargetreleaseextendazurebinding.md) | :heavy_check_mark:                                                                                                           | Generic binding configuration for permissions                                                                                |
| `grant`                                                                                                                      | [models.SyncAcquireResponseTargetReleaseExtendAzureGrant](../models/syncacquireresponsetargetreleaseextendazuregrant.md)     | :heavy_check_mark:                                                                                                           | Grant permissions for a specific cloud platform                                                                              |