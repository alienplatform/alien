# SyncListResponseProfileGcp

GCP-specific platform permission configuration

## Example Usage

```typescript
import { SyncListResponseProfileGcp } from "@alienplatform/platform-api/models";

let value: SyncListResponseProfileGcp = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `binding`                                                                                  | [models.SyncListResponseProfileGcpBinding](../models/synclistresponseprofilegcpbinding.md) | :heavy_check_mark:                                                                         | Generic binding configuration for permissions                                              |
| `grant`                                                                                    | [models.SyncListResponseProfileGcpGrant](../models/synclistresponseprofilegcpgrant.md)     | :heavy_check_mark:                                                                         | Grant permissions for a specific cloud platform                                            |