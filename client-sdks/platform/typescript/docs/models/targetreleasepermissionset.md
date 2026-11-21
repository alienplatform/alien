# TargetReleasePermissionSet

A permission set that can be applied across different cloud platforms

## Example Usage

```typescript
import { TargetReleasePermissionSet } from "@alienplatform/platform-api/models";

let value: TargetReleasePermissionSet = {
  description:
    "perfectly a provided near blink democratize mystify millet sushi dispense",
  id: "<id>",
  platforms: {},
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `description`                                                                                  | *string*                                                                                       | :heavy_check_mark:                                                                             | Human-readable description of what this permission set allows                                  |
| `id`                                                                                           | *string*                                                                                       | :heavy_check_mark:                                                                             | Unique identifier for the permission set (e.g., "storage/data-read")                           |
| `platforms`                                                                                    | [models.TargetReleasePermissionSetPlatforms](../models/targetreleasepermissionsetplatforms.md) | :heavy_check_mark:                                                                             | Platform-specific permission configurations                                                    |