# DataSettingUpPlatformContext

## Example Usage

```typescript
import { DataSettingUpPlatformContext } from "@alienplatform/platform-api/models";

let value: DataSettingUpPlatformContext = {
  platformName: "<value>",
  type: "SettingUpPlatformContext",
};
```

## Fields

| Field                                     | Type                                      | Required                                  | Description                               |
| ----------------------------------------- | ----------------------------------------- | ----------------------------------------- | ----------------------------------------- |
| `platformName`                            | *string*                                  | :heavy_check_mark:                        | Name of the platform (e.g., "AWS", "GCP") |
| `type`                                    | *"SettingUpPlatformContext"*              | :heavy_check_mark:                        | N/A                                       |