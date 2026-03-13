# SyncAcquireResponseCurrentReleaseOverrideAw

AWS-specific platform permission configuration

## Example Usage

```typescript
import { SyncAcquireResponseCurrentReleaseOverrideAw } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseCurrentReleaseOverrideAw = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                        | Type                                                                                                                         | Required                                                                                                                     | Description                                                                                                                  |
| ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                    | [models.SyncAcquireResponseCurrentReleaseOverrideAwBinding](../models/syncacquireresponsecurrentreleaseoverrideawbinding.md) | :heavy_check_mark:                                                                                                           | Generic binding configuration for permissions                                                                                |
| `grant`                                                                                                                      | [models.SyncAcquireResponseCurrentReleaseOverrideAwGrant](../models/syncacquireresponsecurrentreleaseoverrideawgrant.md)     | :heavy_check_mark:                                                                                                           | Grant permissions for a specific cloud platform                                                                              |