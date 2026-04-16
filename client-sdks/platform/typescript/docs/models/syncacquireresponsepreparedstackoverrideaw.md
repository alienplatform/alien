# SyncAcquireResponsePreparedStackOverrideAw

AWS-specific platform permission configuration

## Example Usage

```typescript
import { SyncAcquireResponsePreparedStackOverrideAw } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponsePreparedStackOverrideAw = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                      | Type                                                                                                                       | Required                                                                                                                   | Description                                                                                                                |
| -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                  | [models.SyncAcquireResponsePreparedStackOverrideAwBinding](../models/syncacquireresponsepreparedstackoverrideawbinding.md) | :heavy_check_mark:                                                                                                         | Generic binding configuration for permissions                                                                              |
| `grant`                                                                                                                    | [models.SyncAcquireResponsePreparedStackOverrideAwGrant](../models/syncacquireresponsepreparedstackoverrideawgrant.md)     | :heavy_check_mark:                                                                                                         | Grant permissions for a specific cloud platform                                                                            |