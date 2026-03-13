# SyncAcquireResponseTargetReleaseExtendAw

AWS-specific platform permission configuration

## Example Usage

```typescript
import { SyncAcquireResponseTargetReleaseExtendAw } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponseTargetReleaseExtendAw = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                  | Type                                                                                                                   | Required                                                                                                               | Description                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                              | [models.SyncAcquireResponseTargetReleaseExtendAwBinding](../models/syncacquireresponsetargetreleaseextendawbinding.md) | :heavy_check_mark:                                                                                                     | Generic binding configuration for permissions                                                                          |
| `grant`                                                                                                                | [models.SyncAcquireResponseTargetReleaseExtendAwGrant](../models/syncacquireresponsetargetreleaseextendawgrant.md)     | :heavy_check_mark:                                                                                                     | Grant permissions for a specific cloud platform                                                                        |