# SyncAcquireResponsePreparedStackExtendAw

AWS-specific platform permission configuration

## Example Usage

```typescript
import { SyncAcquireResponsePreparedStackExtendAw } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponsePreparedStackExtendAw = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                  | Type                                                                                                                   | Required                                                                                                               | Description                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                              | [models.SyncAcquireResponsePreparedStackExtendAwBinding](../models/syncacquireresponsepreparedstackextendawbinding.md) | :heavy_check_mark:                                                                                                     | Generic binding configuration for permissions                                                                          |
| `grant`                                                                                                                | [models.SyncAcquireResponsePreparedStackExtendAwGrant](../models/syncacquireresponsepreparedstackextendawgrant.md)     | :heavy_check_mark:                                                                                                     | Grant permissions for a specific cloud platform                                                                        |