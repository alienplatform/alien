# SyncListResponseProfileAw

AWS-specific platform permission configuration

## Example Usage

```typescript
import { SyncListResponseProfileAw } from "@alienplatform/platform-api/models";

let value: SyncListResponseProfileAw = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                    | Type                                                                                     | Required                                                                                 | Description                                                                              |
| ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `binding`                                                                                | [models.SyncListResponseProfileAwBinding](../models/synclistresponseprofileawbinding.md) | :heavy_check_mark:                                                                       | Generic binding configuration for permissions                                            |
| `effect`                                                                                 | [models.SyncListResponseProfileEffect](../models/synclistresponseprofileeffect.md)       | :heavy_minus_sign:                                                                       | IAM effect. Defaults to Allow.                                                           |
| `grant`                                                                                  | [models.SyncListResponseProfileAwGrant](../models/synclistresponseprofileawgrant.md)     | :heavy_check_mark:                                                                       | Grant permissions for a specific cloud platform                                          |