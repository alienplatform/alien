# ProfileReleaseInfo

A permission set that can be applied across different cloud platforms

## Example Usage

```typescript
import { ProfileReleaseInfo } from "@aliendotdev/platform-api/models";

let value: ProfileReleaseInfo = {
  description: "however concerning unless eyebrow",
  id: "<id>",
  platforms: {},
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `description`                                                                  | *string*                                                                       | :heavy_check_mark:                                                             | Human-readable description of what this permission set allows                  |
| `id`                                                                           | *string*                                                                       | :heavy_check_mark:                                                             | Unique identifier for the permission set (e.g., "storage/data-read")           |
| `platforms`                                                                    | [models.ProfileReleaseInfoPlatforms](../models/profilereleaseinfoplatforms.md) | :heavy_check_mark:                                                             | Platform-specific permission configurations                                    |