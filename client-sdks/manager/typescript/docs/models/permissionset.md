# PermissionSet

A permission set that can be applied across different cloud platforms

## Example Usage

```typescript
import { PermissionSet } from "@alienplatform/manager-api/models";

let value: PermissionSet = {
  description: "stiffen because uh-huh cheerfully wonderful display following",
  id: "<id>",
  platforms: {},
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `description`                                                        | *string*                                                             | :heavy_check_mark:                                                   | Human-readable description of what this permission set allows        |
| `id`                                                                 | *string*                                                             | :heavy_check_mark:                                                   | Unique identifier for the permission set (e.g., "storage/data-read") |
| `platforms`                                                          | [models.PlatformPermissions](../models/platformpermissions.md)       | :heavy_check_mark:                                                   | Platform-specific permission configurations                          |