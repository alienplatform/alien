# SyncListResponseExtendAw

AWS-specific platform permission configuration

## Example Usage

```typescript
import { SyncListResponseExtendAw } from "@alienplatform/platform-api/models";

let value: SyncListResponseExtendAw = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                  | Type                                                                                   | Required                                                                               | Description                                                                            |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `binding`                                                                              | [models.SyncListResponseExtendAwBinding](../models/synclistresponseextendawbinding.md) | :heavy_check_mark:                                                                     | Generic binding configuration for permissions                                          |
| `effect`                                                                               | [models.SyncListResponseExtendEffect](../models/synclistresponseextendeffect.md)       | :heavy_minus_sign:                                                                     | IAM effect. Defaults to Allow.                                                         |
| `grant`                                                                                | [models.SyncListResponseExtendAwGrant](../models/synclistresponseextendawgrant.md)     | :heavy_check_mark:                                                                     | Grant permissions for a specific cloud platform                                        |