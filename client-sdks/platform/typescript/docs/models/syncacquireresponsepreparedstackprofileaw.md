# SyncAcquireResponsePreparedStackProfileAw

AWS-specific platform permission configuration

## Example Usage

```typescript
import { SyncAcquireResponsePreparedStackProfileAw } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponsePreparedStackProfileAw = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                    | Type                                                                                                                     | Required                                                                                                                 | Description                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| `binding`                                                                                                                | [models.SyncAcquireResponsePreparedStackProfileAwBinding](../models/syncacquireresponsepreparedstackprofileawbinding.md) | :heavy_check_mark:                                                                                                       | Generic binding configuration for permissions                                                                            |
| `grant`                                                                                                                  | [models.SyncAcquireResponsePreparedStackProfileAwGrant](../models/syncacquireresponsepreparedstackprofileawgrant.md)     | :heavy_check_mark:                                                                                                       | Grant permissions for a specific cloud platform                                                                          |