# SyncListResponseOverride

A permission set that can be applied across different cloud platforms

## Example Usage

```typescript
import { SyncListResponseOverride } from "@alienplatform/platform-api/models";

let value: SyncListResponseOverride = {
  description: "singe excepting censor overcooked gee youthfully um including",
  id: "<id>",
  platforms: {},
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `description`                                                                              | *string*                                                                                   | :heavy_check_mark:                                                                         | Human-readable description of what this permission set allows                              |
| `id`                                                                                       | *string*                                                                                   | :heavy_check_mark:                                                                         | Unique identifier for the permission set (e.g., "storage/data-read")                       |
| `platforms`                                                                                | [models.SyncListResponseOverridePlatforms](../models/synclistresponseoverrideplatforms.md) | :heavy_check_mark:                                                                         | Platform-specific permission configurations                                                |