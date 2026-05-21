# SyncListResponseOverrideGcp

GCP-specific platform permission configuration

## Example Usage

```typescript
import { SyncListResponseOverrideGcp } from "@alienplatform/platform-api/models";

let value: SyncListResponseOverrideGcp = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `binding`                                                                                    | [models.SyncListResponseOverrideGcpBinding](../models/synclistresponseoverridegcpbinding.md) | :heavy_check_mark:                                                                           | Generic binding configuration for permissions                                                |
| `grant`                                                                                      | [models.SyncListResponseOverrideGcpGrant](../models/synclistresponseoverridegcpgrant.md)     | :heavy_check_mark:                                                                           | Grant permissions for a specific cloud platform                                              |