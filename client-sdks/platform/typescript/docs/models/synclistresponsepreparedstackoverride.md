# SyncListResponsePreparedStackOverride

A permission set that can be applied across different cloud platforms

## Example Usage

```typescript
import { SyncListResponsePreparedStackOverride } from "@alienplatform/platform-api/models";

let value: SyncListResponsePreparedStackOverride = {
  description:
    "circa as armchair jiggle fictionalize devise consequently acceptable concerning fraternise",
  id: "<id>",
  platforms: {},
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `description`                                                                                                        | *string*                                                                                                             | :heavy_check_mark:                                                                                                   | Human-readable description of what this permission set allows                                                        |
| `id`                                                                                                                 | *string*                                                                                                             | :heavy_check_mark:                                                                                                   | Unique identifier for the permission set (e.g., "storage/data-read")                                                 |
| `platforms`                                                                                                          | [models.SyncListResponsePreparedStackOverridePlatforms](../models/synclistresponsepreparedstackoverrideplatforms.md) | :heavy_check_mark:                                                                                                   | Platform-specific permission configurations                                                                          |
