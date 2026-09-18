# CurrentReleaseExtend

A permission set that can be applied across different cloud platforms

## Example Usage

```typescript
import { CurrentReleaseExtend } from "@alienplatform/platform-api/models";

let value: CurrentReleaseExtend = {
  description: "eek yuck suddenly soupy give cease",
  id: "<id>",
  platforms: {},
};
```

## Fields

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `description`                                                                      | *string*                                                                           | :heavy_check_mark:                                                                 | Human-readable description of what this permission set allows                      |
| `id`                                                                               | *string*                                                                           | :heavy_check_mark:                                                                 | Unique identifier for the permission set (e.g., "storage/data-read")               |
| `platforms`                                                                        | [models.CurrentReleaseExtendPlatforms](../models/currentreleaseextendplatforms.md) | :heavy_check_mark:                                                                 | Platform-specific permission configurations                                        |