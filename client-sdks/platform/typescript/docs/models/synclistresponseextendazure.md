# SyncListResponseExtendAzure

Azure-specific platform permission configuration

## Example Usage

```typescript
import { SyncListResponseExtendAzure } from "@alienplatform/platform-api/models";

let value: SyncListResponseExtendAzure = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `binding`                                                                                    | [models.SyncListResponseExtendAzureBinding](../models/synclistresponseextendazurebinding.md) | :heavy_check_mark:                                                                           | Generic binding configuration for permissions                                                |
| `grant`                                                                                      | [models.SyncListResponseExtendAzureGrant](../models/synclistresponseextendazuregrant.md)     | :heavy_check_mark:                                                                           | Grant permissions for a specific cloud platform                                              |