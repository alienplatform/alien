# SyncListResponseOverrideAzure

Azure-specific platform permission configuration

## Example Usage

```typescript
import { SyncListResponseOverrideAzure } from "@alienplatform/platform-api/models";

let value: SyncListResponseOverrideAzure = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `binding`                                                                                        | [models.SyncListResponseOverrideAzureBinding](../models/synclistresponseoverrideazurebinding.md) | :heavy_check_mark:                                                                               | Generic binding configuration for permissions                                                    |
| `grant`                                                                                          | [models.SyncListResponseOverrideAzureGrant](../models/synclistresponseoverrideazuregrant.md)     | :heavy_check_mark:                                                                               | Grant permissions for a specific cloud platform                                                  |